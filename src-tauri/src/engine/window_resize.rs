//! 无边框窗口边缘拉伸（GTK 层）
//!
//! 背景：`decorations: false` 的无边框窗口，GTK/GNOME 不提供窗口边缘的 resize 热区，
//! 拖边无法改变窗口大小。
//!
//! 做法：在窗口最上层铺一圈**只占边缘 `EDGE` 像素的原生透明部件**（4 条边 + 4 个角），
//! 接管边缘区域的鼠标事件——悬停切换 resize 光标，左键按下调用 `begin_resize_drag`
//! 交给窗口管理器拉伸。中间区域不铺部件，事件照常穿透到网页 / Vue UI。
//!
//! 为什么不用 GDK 全局事件过滤器（`gdk_window_add_filter`）：Wayland 会话下实测过滤器
//! 收不到任何事件（GDK Wayland 后端不走该路径），改用 GTK 部件事件可靠（网页能收到点击
//! 即证明部件事件链路正常）。

#![cfg(target_os = "linux")]

use gtk::gdk::WindowEdge;
use gtk::prelude::*;

/// 边缘热区宽度（px）
const EDGE: i32 = 6;

/// 8 个方向规格：(方向, 光标名, 水平对齐, 垂直对齐)
/// 先加 4 条边、后加 4 个角——角在上层，重叠处优先触发斜向拉伸
const SPECS: [(WindowEdge, &str, gtk::Align, gtk::Align); 8] = [
    (WindowEdge::North, "n-resize", gtk::Align::Fill, gtk::Align::Start),
    (WindowEdge::South, "s-resize", gtk::Align::Fill, gtk::Align::End),
    (WindowEdge::West, "w-resize", gtk::Align::Start, gtk::Align::Fill),
    (WindowEdge::East, "e-resize", gtk::Align::End, gtk::Align::Fill),
    (WindowEdge::NorthWest, "nw-resize", gtk::Align::Start, gtk::Align::Start),
    (WindowEdge::NorthEast, "ne-resize", gtk::Align::End, gtk::Align::Start),
    (WindowEdge::SouthWest, "sw-resize", gtk::Align::Start, gtk::Align::End),
    (WindowEdge::SouthEast, "se-resize", gtk::Align::End, gtk::Align::End),
];

/// 安装边缘拉伸（setup 阶段、GTK 主线程调用）
pub fn install(overlay: &gtk::Overlay, gtk_window: &gtk::Window) {
    // 边缘条只负责收事件，不绘制——CSS 置空背景，避免遮挡网页边缘
    if let Some(screen) = gtk::gdk::Screen::default() {
        let css = gtk::CssProvider::new();
        if css
            .load_from_data(b"#resize-edge { background: none; border: none; }")
            .is_ok()
        {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    let display = gtk::gdk::Display::default();
    for (edge, cursor_name, halign, valign) in SPECS {
        let eb = gtk::EventBox::new();
        eb.set_widget_name("resize-edge");
        eb.set_halign(halign);
        eb.set_valign(valign);
        // 只约束"边缘方向"的那一维，另一维由 Fill 撑开
        match (halign, valign) {
            (gtk::Align::Fill, _) => eb.set_size_request(-1, EDGE),
            (_, gtk::Align::Fill) => eb.set_size_request(EDGE, -1),
            _ => eb.set_size_request(EDGE, EDGE),
        }
        eb.add_events(
            gtk::gdk::EventMask::POINTER_MOTION_MASK
                | gtk::gdk::EventMask::BUTTON_PRESS_MASK
                | gtk::gdk::EventMask::ENTER_NOTIFY_MASK,
        );

        // 悬停切换 resize 光标
        let cursor = display
            .as_ref()
            .and_then(|d| gtk::gdk::Cursor::from_name(d, cursor_name));
        let cursor_enter = cursor.clone();
        eb.connect_enter_notify_event(move |w, _| {
            if let Some(gw) = w.window() {
                gw.set_cursor(cursor_enter.as_ref());
            }
            gtk::glib::Propagation::Proceed
        });

        // 左键按下 → 交给窗口管理器拉伸
        let win = gtk_window.clone();
        eb.connect_button_press_event(move |_, ev| {
            if ev.button() == 1 {
                win.begin_resize_drag(
                    edge,
                    1,
                    ev.root().0 as i32,
                    ev.root().1 as i32,
                    ev.time(),
                );
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });

        eb.show();
        overlay.add_overlay(&eb);
    }

    log::info!(
        "[window-resize] 无边框窗口边缘拉伸已安装（热区 {}px，8 方向原生部件）",
        EDGE
    );
}
