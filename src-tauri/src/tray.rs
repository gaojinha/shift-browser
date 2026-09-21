//! 系统托盘（StatusNotifierItem / AppIndicator）
//!
//! 宿主：GNOME 的 ubuntu-appindicators 扩展（提供 org.kde.StatusNotifierWatcher），
//! 走 ksni 纯 Rust D-Bus 实现，不引入 libappindicator C 依赖（符合本仓"少引原生依赖"原则）。
//!
//! 行为（高哥钦定）：
//! - 点窗口关闭按钮 → 缩到托盘，不退出（见 lib.rs 的 delete-event 拦截）
//! - 托盘左键单击 → 显示/隐藏主窗口
//! - 托盘菜单：显示/隐藏窗口 · 下载管理 · 密码库 · 退出
//! - 真退出只走「退出」菜单或前端退出按钮（置 REAL_QUIT 后放行 delete-event）
//!
//! 线程模型（重要）：ksni 的菜单闭包跑在 D-Bus 线程，**绝不能直接碰 GTK**。
//! 闭包只往 mpsc 投递 [`TrayAction`]，由 GTK 主线程的 glib 定时器排空并执行。

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::OnceLock;

/// 真退出标志：置位后 delete-event 放行（不再缩托盘）。
/// 前端退出按钮 / 托盘「退出」菜单都会先置位再关窗。
pub static REAL_QUIT: AtomicBool = AtomicBool::new(false);

/// 应用图标（编译期嵌入 128x128 源图，不依赖运行时路径）
const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

/// 托盘图标缓存（多尺寸，宿主按需挑；缩放/解码只做一次）
static TRAY_ICONS: OnceLock<Vec<ksni::Icon>> = OnceLock::new();

/// 把嵌入的 PNG 解码 + 缩放到托盘常用尺寸，转成 ksni 要的 ARGB32（network byte order）。
/// gdk-pixbuf 读出的像素是 RGBA，需重排成 A,R,G,B 大端序。
fn tray_icons() -> &'static [ksni::Icon] {
    TRAY_ICONS.get_or_init(|| {
        use gtk::prelude::*;
        let loader = gtk::gdk_pixbuf::PixbufLoader::new();
        if let Err(e) = loader.write(ICON_PNG) {
            log::warn!("[tray] 图标写入失败: {}", e);
            return Vec::new();
        }
        if let Err(e) = loader.close() {
            log::warn!("[tray] 图标解析失败: {}", e);
            return Vec::new();
        }
        let src = match loader.pixbuf() {
            Some(p) => p,
            None => {
                log::warn!("[tray] 图标解码为空");
                return Vec::new();
            }
        };

        // GNOME 顶栏常用 22px；同时给 16/32 让宿主自己挑
        let mut out = Vec::new();
        for size in [16, 22, 32] {
            let pb = src
                .scale_simple(size, size, gtk::gdk_pixbuf::InterpType::Bilinear)
                .unwrap_or_else(|| src.clone());
            let (w, h) = (pb.width(), pb.height());
            let n = pb.n_channels() as usize;
            let stride = pb.rowstride() as usize;
            let bytes = pb.read_pixel_bytes();
            let mut data = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let o = y as usize * stride + x as usize * n;
                    let r = bytes[o];
                    let g = bytes[o + 1];
                    let b = bytes[o + 2];
                    let a = if n == 4 { bytes[o + 3] } else { 255 };
                    // ARGB32，network byte order（大端 A,R,G,B）
                    data.extend_from_slice(&[a, r, g, b]);
                }
            }
            out.push(ksni::Icon {
                width: w,
                height: h,
                data,
            });
        }
        log::info!("[tray] 应用图标已加载（{} 个尺寸）", out.len());
        out
    })
}

/// 托盘动作（D-Bus 线程 → GTK 主线程）
#[derive(Debug, Clone)]
pub enum TrayAction {
    /// 显示/隐藏主窗口
    ToggleWindow,
    /// 打开下载管理面板
    OpenDownloads,
    /// 打开密码库面板
    OpenPasswords,
    /// 真退出
    Quit,
}

/// 托盘图标实体（实现 ksni::Tray）
pub struct ShiftTray {
    tx: Sender<TrayAction>,
}

impl ShiftTray {
    pub fn new(tx: Sender<TrayAction>) -> Self {
        Self { tx }
    }
}

impl ksni::Tray for ShiftTray {
    fn id(&self) -> String {
        "shift-browser".into()
    }

    fn title(&self) -> String {
        "Shift Browser".into()
    }

    /// 托盘图标 = 应用自己的图标（见 tray_icons，ARGB32 多尺寸）
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        tray_icons().to_vec()
    }

    /// 左键单击 = 显示/隐藏窗口
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayAction::ToggleWindow);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        vec![
            StandardItem {
                label: "显示/隐藏窗口".into(),
                activate: Box::new(|t: &mut ShiftTray| {
                    let _ = t.tx.send(TrayAction::ToggleWindow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "下载管理".into(),
                activate: Box::new(|t: &mut ShiftTray| {
                    let _ = t.tx.send(TrayAction::OpenDownloads);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "密码库".into(),
                activate: Box::new(|t: &mut ShiftTray| {
                    let _ = t.tx.send(TrayAction::OpenPasswords);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出".into(),
                activate: Box::new(|t: &mut ShiftTray| {
                    let _ = t.tx.send(TrayAction::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}
