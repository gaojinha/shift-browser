# 性能基线报告（R1-8）

- 采集时间：2026-09-25 19:28:07 +0800
- 代码版本：5.8.0（commit 2971e37，工作树含 R1 批次未提交改动）
- 机器：Linux 6.17.0-41-generic x86_64
- CPU：Intel(R) Core(TM) i3-7100 CPU @ 3.90GHz（4 核）
- 内存：22.8 GiB
- 图形会话：wayland（DISPLAY=:0）
- 采集脚本：`scripts/perf-baseline.sh`（同机 3 次取中位数）

## 结果

| 指标 | 全部原始值（3 轮） | 中位数（基线） |
|---|---|---|
| 冷启动（空 profile，进程起 → onReady 完成） | 2.618 / 2.522 / 2.229s | **2.522s** |
| 恢复 20 标签（shift://newtab 离线页） | 8.387 / 6.877 / 6.870s | **6.877s** |
| 恢复 10 标签耗时 | 3.962 / 3.663 / 4.251s | **3.962s** |
| 恢复 10 标签稳态 RSS（主+WebKit 全进程组，settle 8s） | 2519824 / 2515872 / 2502192KB | **2456.9 MiB** |
| deb 安装包体积（5.8.0） | — | **8.45 MiB** |

## 方法与口径

1. **隔离**：每轮使用独立 mktemp XDG profile（data/config/cache/runtime 全重定向），
   不触碰真实用户数据；轮间进程整组 SIGTERM/SIGKILL，single-instance 串行无冲突。
2. **就绪信号**：前端 `onReady 完成 标签数=N` 经 front_log 由 tauri-plugin-log 同时
   写入 LogDir 文件与进程 stdout；LogDir 默认阈值仅 40KB（KeepOne，超限删除重建），
   多标签恢复时信号行会随旧文件永久丢失，故脚本以【不滚动的 stdout 落盘文件】为主信号源、
   LogDir 为兜底，在每次启动前挂 `tail -F` 流式监听，命中即停表；
   表值 = 进程启动到该信号的墙钟时间。
3. **恢复场景**：先空库启动建表并退出（等进程组完全退出，保证其退出保存已提交），
   清除默认工作区旧的"自动保存"记录后，插入 N 个 `shift://newtab` 内置页的会话
   （零网络依赖、结果可复现）并回读校验标签数，再二次启动计时。
4. **RSS**：按进程树汇总主进程与全部 WebKitWebProcess/NetworkProcess 的 /proc RSS，
   onReady 后再等 8s 取稳态值。
5. **局限**：内置页内存占用低于真实网页，RSS 基线用于版本间回归对比，不代表重网页场景；
   冷启动含桌面会话抖动，跨机器比较无意义，仅同机回归有效。

## 复现

```bash
scripts/perf-baseline.sh             # 自动 release 构建 + 3 轮测量，覆盖输出本文件
scripts/perf-baseline.sh --skip-build
```
