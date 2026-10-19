// webview 显隐控制（P1-9）单测
// 验证 seq 令牌单调递增 + 参数透传 + 失败静默 + 菜单底垫（vitest 起步测试）
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createPinia, setActivePinia } from "pinia";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

import { invoke } from "@tauri-apps/api/core";

function visibleCalls() {
  return vi
    .mocked(invoke)
    .mock.calls.filter((c) => c[0] === "set_webview_visible");
}
function backdropCalls() {
  return vi
    .mocked(invoke)
    .mock.calls.filter((c) => c[0] === "page_backdrop_shot");
}
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

describe("setWebviewVisible", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    setActivePinia(createPinia());
    // 重置模块内 seq（每次测试从 1 开始，断言更直观）
    vi.resetModules();
  });

  it("seq 单调递增并透传参数", async () => {
    const { setWebviewVisible: swv } = await import("./webview");
    await swv(false, "test-a");
    await swv(false, "test-b");
    // 第二个隐藏请求仍会下发（保持隐藏，刷新 seq）；首个隐藏另抓一次底垫快照
    const calls = visibleCalls();
    expect(calls).toHaveLength(2);
    expect(calls[0][1]).toMatchObject({ visible: false, seq: 1, source: "test-a" });
    expect(calls[1][1]).toMatchObject({ visible: false, seq: 2, source: "test-b" });
    expect(backdropCalls()).toHaveLength(1);
  });

  it("引用计数：还有覆盖层时关闭一个不恢复显示", async () => {
    const mod = await import("./webview");
    mod.resetWebviewHiders();
    vi.mocked(invoke).mockClear();
    await mod.setWebviewVisible(false, "a");
    await mod.setWebviewVisible(false, "b");
    await mod.setWebviewVisible(true, "a");
    // a 关了但 b 还压着，不应发 visible:true
    expect(visibleCalls()).toHaveLength(2);
    expect(mod.webviewHiderCount()).toBe(1);
    await mod.setWebviewVisible(true, "b");
    expect(visibleCalls()).toHaveLength(3);
    expect(mod.webviewHiderCount()).toBe(0);
  });

  it("invoke 失败时静默吞掉（不抛异常）", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    const { setWebviewVisible: swv } = await import("./webview");
    await expect(swv(true)).resolves.toBeUndefined();
  });

  it("invoke 失败自动重试，且复用同一 seq（幂等，不会被后端重复应用）", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    const mod = await import("./webview");
    mod.resetWebviewHiders();
    await mod.setWebviewVisible(true, "retry-test");
    const calls = visibleCalls();
    expect(calls).toHaveLength(2);
    expect(calls[0][1]).toMatchObject({ visible: true, seq: 1, source: "retry-test" });
    expect(calls[1][1]).toMatchObject({ visible: true, seq: 1, source: "retry-test" });
  });

  it("底垫：首个隐藏抓快照、清零即撤", async () => {
    vi.mocked(invoke).mockImplementation(((cmd: string) =>
      cmd === "page_backdrop_shot"
        ? Promise.resolve("data:image/png;base64,AAA")
        : Promise.resolve(undefined)) as never);
    const mod = await import("./webview");
    const { useBackdropStore } = await import("../stores/backdrop");
    mod.resetWebviewHiders();
    expect(useBackdropStore().image).toBeNull();
    await mod.setWebviewVisible(false, "menu");
    await flush();
    expect(useBackdropStore().image).toBe("data:image/png;base64,AAA");
    await mod.setWebviewVisible(true, "menu");
    expect(useBackdropStore().image).toBeNull();
  });

  it("底垫：菜单关得比快照快，晚到的快照直接丢掉", async () => {
    let release!: (v: string) => void;
    const gate = new Promise<string>((r) => (release = r));
    vi.mocked(invoke).mockImplementation(((cmd: string) =>
      cmd === "page_backdrop_shot" ? gate : Promise.resolve(undefined)) as never);
    const mod = await import("./webview");
    const { useBackdropStore } = await import("../stores/backdrop");
    mod.resetWebviewHiders();
    const hideP = mod.setWebviewVisible(false, "menu");
    await mod.setWebviewVisible(true, "menu");
    release("data:image/png;base64,LATE");
    await hideP;
    await flush();
    // 覆盖层已清零，晚到的快照不得盖上来
    expect(useBackdropStore().image).toBeNull();
  });

  it("底垫：暖缓存命中则零闪烁（隐藏时不再抓）", async () => {
    vi.mocked(invoke).mockImplementation(((cmd: string) =>
      cmd === "page_backdrop_shot"
        ? Promise.resolve("data:image/png;base64,WARM")
        : Promise.resolve(undefined)) as never)
    const mod = await import("./webview")
    const { useBackdropStore } = await import("../stores/backdrop")
    const { useTabsStore } = await import("../stores/tabs")
    mod.resetWebviewHiders()
    const tabs = useTabsStore()
    tabs.tabs = [
      {
        id: "a", url: "https://a.com/", title: "a", favicon: "",
        is_active: true, is_pinned: false, is_muted: false,
        hibernated: false, private: false, groupId: null,
      },
    ]
    tabs.activeTabId = "a"
    mod.warmBackdrop()
    await flush()
    vi.mocked(invoke).mockClear()
    await mod.setWebviewVisible(false, "menu")
    expect(vi.mocked(invoke).mock.calls.filter((c) => c[0] === "page_backdrop_shot")).toHaveLength(0)
    expect(useBackdropStore().image).toBe("data:image/png;base64,WARM")
    await mod.setWebviewVisible(true, "menu")
    expect(useBackdropStore().image).toBeNull()
  })

  it("底垫：切标签后缓存键对不上，重新现抓", async () => {
    vi.mocked(invoke).mockImplementation(((cmd: string) =>
      cmd === "page_backdrop_shot"
        ? Promise.resolve("data:image/png;base64,SHOT")
        : Promise.resolve(undefined)) as never)
    const mod = await import("./webview")
    const { useBackdropStore } = await import("../stores/backdrop")
    const { useTabsStore } = await import("../stores/tabs")
    mod.resetWebviewHiders()
    const tabs = useTabsStore()
    const mk = (id: string, url: string) => ({
      id, url, title: id, favicon: "",
      is_active: false, is_pinned: false, is_muted: false,
      hibernated: false, private: false, groupId: null,
    })
    tabs.tabs = [mk("a", "https://a.com/"), mk("b", "https://b.com/")]
    tabs.activeTabId = "a"
    mod.warmBackdrop()
    await flush()
    tabs.activeTabId = "b"
    vi.mocked(invoke).mockClear()
    await mod.setWebviewVisible(false, "menu")
    await flush()
    expect(vi.mocked(invoke).mock.calls.filter((c) => c[0] === "page_backdrop_shot")).toHaveLength(1)
    expect(useBackdropStore().image).toBe("data:image/png;base64,SHOT")
  })
})
