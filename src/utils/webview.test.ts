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
    await swv(true, "test");
    await swv(false, "test");
    const calls = vi.mocked(invoke).mock.calls;
    expect(calls).toHaveLength(2);
    expect(calls[0][1]).toMatchObject({ visible: true, seq: 1, source: "test" });
    expect(calls[1][1]).toMatchObject({ visible: false, seq: 2, source: "test" });
  });

  it("invoke 失败时静默吞掉（不抛异常）", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    const { setWebviewVisible: swv } = await import("./webview");
    await expect(swv(true)).resolves.toBeUndefined();
  });
});