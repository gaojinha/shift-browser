// webview 显隐控制（P1-9）单测
// 验证 seq 令牌单调递增 + 参数透传 + 失败静默（vitest 起步测试）
import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

import { invoke } from "@tauri-apps/api/core";

describe("setWebviewVisible", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    // 重置模块内 seq（每次测试从 1 开始，断言更直观）
    vi.resetModules();
  });

  it("seq 单调递增并透传参数", async () => {
    const { setWebviewVisible: swv } = await import("./webview");
    await swv(false, "test-a");
    await swv(false, "test-b");
    // 第二个隐藏请求仍会下发（保持隐藏，刷新 seq）
    const calls = vi.mocked(invoke).mock.calls;
    expect(calls).toHaveLength(2);
    expect(calls[0][1]).toMatchObject({ visible: false, seq: 1, source: "test-a" });
    expect(calls[1][1]).toMatchObject({ visible: false, seq: 2, source: "test-b" });
  });

  it("引用计数：还有覆盖层时关闭一个不恢复显示", async () => {
    const mod = await import("./webview");
    mod.resetWebviewHiders();
    vi.mocked(invoke).mockClear();
    await mod.setWebviewVisible(false, "a");
    await mod.setWebviewVisible(false, "b");
    await mod.setWebviewVisible(true, "a");
    // a 关了但 b 还压着，不应发 visible:true
    const calls = vi.mocked(invoke).mock.calls;
    expect(calls).toHaveLength(2);
    expect(mod.webviewHiderCount()).toBe(1);
    await mod.setWebviewVisible(true, "b");
    expect(vi.mocked(invoke).mock.calls).toHaveLength(3);
    expect(mod.webviewHiderCount()).toBe(0);
  });

  it("invoke 失败时静默吞掉（不抛异常）", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    const { setWebviewVisible: swv } = await import("./webview");
    await expect(swv(true)).resolves.toBeUndefined();
  });
});
