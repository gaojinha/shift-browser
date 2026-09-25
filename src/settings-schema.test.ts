// 批 2「孤儿键补 UI」契约防漂移：8 个既有 settings 键必须在生成的 SETTINGS_SCHEMA 中，
// 且 kind 与组件绑定方式一致（bool→开关、str/int→输入）。
// 前端无组件挂载测试基建（无 @vue/test-utils/jsdom），防漂移落在契约层。
import { describe, it, expect } from "vitest";
import { SETTINGS_SCHEMA } from "./constants";

describe("批 2 新增设置控件绑定的 8 个键", () => {
  const boolKeys = ["show_lunar", "show_quote", "show_weather", "api_readonly", "api_agent_actions"];
  const strKeys = ["theme_color", "wallpaper", "weather_city"];
  const intKeys = ["tab_hibernate_count"];

  it("布尔键存在且 kind=bool、默认值合法", () => {
    for (const k of boolKeys) {
      const def = SETTINGS_SCHEMA[k];
      expect(def, `缺少契约定义: ${k}`).toBeTruthy();
      expect(def.kind).toBe("bool");
      expect(["true", "false"]).toContain(def.default);
    }
  });

  it("字符串键存在且 kind=str", () => {
    for (const k of strKeys) {
      const def = SETTINGS_SCHEMA[k];
      expect(def, `缺少契约定义: ${k}`).toBeTruthy();
      expect(def.kind).toBe("str");
    }
  });

  it("休眠数量阈值存在且 kind=int、默认 0（关闭阈值）", () => {
    for (const k of intKeys) {
      const def = SETTINGS_SCHEMA[k];
      expect(def, `缺少契约定义: ${k}`).toBeTruthy();
      expect(def.kind).toBe("int");
    }
    expect(SETTINGS_SCHEMA.tab_hibernate_count.default).toBe("0");
  });
});
