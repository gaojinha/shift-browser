// settings store + useSettingsSet 写入通道单测（R1 测试补课）
// 契约：get 默认值回退 / loadSettings 失败兜底 / set 失败回滚 / onSet 副作用命令
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useSettingsStore } from "./settings";
import { DEFAULT_SETTINGS } from "../constants";
import { useSettingsSet } from "../composables/useSettingsSet";

beforeEach(() => {
  setActivePinia(createPinia());
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
});

describe("useSettingsStore", () => {
  it("get: 无记录回退 DEFAULT_SETTINGS，未知 key 回退空串", () => {
    const s = useSettingsStore();
    expect(s.get("theme")).toBe(DEFAULT_SETTINGS.theme);
    expect(s.get("show_bookmarks_bar")).toBe("true");
    expect(s.get("___not_exist___")).toBe("");
  });

  it("loadSettings: 成功时按行填充并置 loaded", async () => {
    invokeMock.mockResolvedValueOnce([
      { key: "theme", value: "light" },
      { key: "custom_only", value: "x" },
    ]);
    const s = useSettingsStore();
    await s.loadSettings();
    expect(invokeMock).toHaveBeenCalledWith("get_all_settings");
    expect(s.loaded).toBe(true);
    expect(s.get("theme")).toBe("light");
    expect(s.get("custom_only")).toBe("x");
    // 未返回的 key 仍回退默认值
    expect(s.get("vertical_tabs")).toBe(DEFAULT_SETTINGS.vertical_tabs);
  });

  it("loadSettings: invoke 失败时空表兜底，不抛出且 loaded=true", async () => {
    invokeMock.mockRejectedValueOnce(new Error("db locked"));
    const s = useSettingsStore();
    await expect(s.loadSettings()).resolves.toBeUndefined();
    expect(s.loaded).toBe(true);
    expect(s.get("theme")).toBe(DEFAULT_SETTINGS.theme);
  });

  it("set: 成功时写内存并按 {key,value} 调 invoke", async () => {
    const s = useSettingsStore();
    await s.set("theme", "light");
    expect(s.get("theme")).toBe("light");
    expect(invokeMock).toHaveBeenCalledWith("set_setting", { key: "theme", value: "light" });
  });

  it("set: invoke 失败时内存值回滚到原值", async () => {
    invokeMock.mockRejectedValueOnce(new Error("disk full"));
    const s = useSettingsStore();
    s.settings.theme = "dark";
    await s.set("theme", "light");
    // 乐观更新后回滚
    expect(s.get("theme")).toBe("dark");
    // 此前无记录的 key 失败后保持 undefined → get 仍回退默认值
    invokeMock.mockRejectedValueOnce(new Error("disk full"));
    await s.set("font_size", "20");
    expect(s.get("font_size")).toBe(DEFAULT_SETTINGS.font_size ?? "");
  });

  it("getters: isLight / showBookmarksBar 按默认值与覆盖值解析布尔", () => {
    const s = useSettingsStore();
    // 默认 dark / 书签栏 true
    expect(s.isLight).toBe(false);
    expect(s.showBookmarksBar).toBe(true);
    s.settings.theme = "light";
    s.settings.show_bookmarks_bar = "false";
    expect(s.isLight).toBe(true);
    expect(s.showBookmarksBar).toBe(false);
  });
});

describe("useSettingsSet.onSet", () => {
  it("普通 key：只落 set_setting，不触发任何副作用命令", async () => {
    const { onSet } = useSettingsSet();
    await onSet("homepage", "https://example.com");
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("set_setting", {
      key: "homepage",
      value: "https://example.com",
    });
  });

  it("show_bookmarks_bar：额外通知 Rust，且值转布尔 visible", async () => {
    const { onSet } = useSettingsSet();
    await onSet("show_bookmarks_bar", "false");
    expect(invokeMock).toHaveBeenCalledWith("set_setting", {
      key: "show_bookmarks_bar",
      value: "false",
    });
    expect(invokeMock).toHaveBeenCalledWith("set_bookmarks_bar_visible", { visible: false });
  });

  it("vertical_tabs：额外通知 Rust 重算几何，active 为布尔", async () => {
    const { onSet } = useSettingsSet();
    await onSet("vertical_tabs", "true");
    expect(invokeMock).toHaveBeenCalledWith("set_vertical_tabs", { active: true });
  });

  it("网络规则三件套：触发 sync_ext_rules", async () => {
    const { onSet } = useSettingsSet();
    await onSet("net_block_rules", "[]");
    expect(invokeMock).toHaveBeenCalledWith("sync_ext_rules");
    invokeMock.mockClear();
    await onSet("gpc_header", "true");
    expect(invokeMock).toHaveBeenCalledWith("sync_ext_rules");
  });

  it("启动页相关 key：触发 refresh_newtab", async () => {
    const { onSet } = useSettingsSet();
    await onSet("search_engine", "google");
    expect(invokeMock).toHaveBeenCalledWith("refresh_newtab");
  });

  it("副作用命令失败不影响主设置写入，且不向上抛", async () => {
    // 第一次 set_setting 成功，第二次副作用失败
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "set_vertical_tabs" ? Promise.reject(new Error("geom fail")) : Promise.resolve(undefined),
    );
    const s = useSettingsStore();
    const { onSet } = useSettingsSet();
    await expect(onSet("vertical_tabs", "true")).resolves.toBeUndefined();
    expect(s.get("vertical_tabs")).toBe("true");
  });
});
