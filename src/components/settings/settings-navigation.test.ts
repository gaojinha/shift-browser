// 设置 IA 注册表防漂移测试（2026-09 重组）
// - 导航结构：4 分组 8 栏、id 唯一
// - 旧 id 映射全覆盖且目标合法
// - 搜索注册表：栏目/页签/锚点合法，锚点必须真实存在于 settings/*.vue 的 data-anchor
// - resolveTarget / searchSettings 纯函数行为
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  SECTION_GROUPS,
  SECTION_IDS,
  SECTION_ANCHORS,
  LEGACY_SECTION_ALIASES,
  SETTING_SEARCH_INDEX,
  resolveTarget,
  searchSettings,
  type SectionId,
} from "./settingsNavigation";

// 收集 settings 目录全部 SFC 中真实存在的 data-anchor
const here = path.dirname(fileURLToPath(import.meta.url));
const vueAnchors = new Set<string>();
for (const f of readdirSync(here).filter((n) => n.endsWith(".vue"))) {
  const src = readFileSync(path.join(here, f), "utf8");
  for (const m of src.matchAll(/data-anchor="([^"]+)"/g)) vueAnchors.add(m[1]);
}
const aiSectionSrc = readFileSync(path.join(here, "AiSection.vue"), "utf8");

describe("SECTION_GROUPS 导航结构", () => {
  it("恰好 4 个分组、8 个栏目，顺序固定", () => {
    expect(SECTION_GROUPS.map((g) => g.label)).toEqual([
      "基础",
      "安全与主权",
      "智能与平台",
      "系统",
    ]);
    expect(SECTION_IDS).toEqual([
      "general",
      "appearance",
      "privacy",
      "data",
      "ai",
      "extensions",
      "advanced",
      "about",
    ]);
  });

  it("栏目 id 唯一且 icon/label 非空", () => {
    const ids = SECTION_GROUPS.flatMap((g) => g.items.map((i) => i.id));
    expect(new Set(ids).size).toBe(ids.length);
    for (const it of SECTION_GROUPS.flatMap((g) => g.items)) {
      expect(it.icon.length).toBeGreaterThan(0);
      expect(it.label.length).toBeGreaterThan(0);
    }
  });
});

describe("LEGACY_SECTION_ALIASES 旧深链映射", () => {
  it("旧 10 个栏目标识全部有映射", () => {
    for (const oldId of [
      "general",
      "appearance",
      "ai",
      "mcp",
      "extensions",
      "privacy",
      "data",
      "api",
      "selfcheck",
      "about",
    ]) {
      expect(LEGACY_SECTION_ALIASES[oldId], `缺少旧 id 映射: ${oldId}`).toBeTruthy();
    }
  });

  it("映射目标栏目/页签/锚点均合法", () => {
    for (const [oldId, t] of Object.entries(LEGACY_SECTION_ALIASES)) {
      expect(SECTION_IDS).toContain(t.section);
      if (t.anchor) expect(SECTION_ANCHORS[t.section]).toContain(t.anchor);
      if (oldId === "mcp") expect(t.aiTab).toBe("mcp");
      if (t.aiTab) expect(["model", "mcp", "labs"]).toContain(t.aiTab);
    }
  });
});

describe("resolveTarget", () => {
  it("空值兜底常规栏", () => {
    expect(resolveTarget(undefined)).toEqual({ section: "general", aiTab: "model", anchor: "" });
    expect(resolveTarget("")).toEqual({ section: "general", aiTab: "model", anchor: "" });
  });

  it("旧一级栏 id：mcp/api/selfcheck 分别落到 AI#MCP、高级#api、高级#selfcheck", () => {
    expect(resolveTarget("mcp")).toEqual({ section: "ai", aiTab: "mcp", anchor: "" });
    expect(resolveTarget("api")).toEqual({ section: "advanced", aiTab: "model", anchor: "api" });
    expect(resolveTarget("selfcheck")).toEqual({
      section: "advanced",
      aiTab: "model",
      anchor: "selfcheck",
    });
  });

  it("section#anchor 形态：privacy#identity / advanced#compat / ai#mcp", () => {
    expect(resolveTarget("privacy#identity")).toMatchObject({
      section: "privacy",
      anchor: "identity",
    });
    expect(resolveTarget("advanced#compat")).toMatchObject({
      section: "advanced",
      anchor: "compat",
    });
    expect(resolveTarget("ai#mcp")).toEqual({ section: "ai", aiTab: "mcp", anchor: "mcp" });
    expect(resolveTarget("ai#labs")).toEqual({ section: "ai", aiTab: "labs", anchor: "labs" });
  });

  it("无 hash 的现行栏目 id 落到栏顶", () => {
    expect(resolveTarget("privacy")).toEqual({ section: "privacy", aiTab: "model", anchor: "" });
    expect(resolveTarget("data")).toMatchObject({ section: "data", anchor: "" });
  });

  it("非法 id 与非法锚点安全兜底", () => {
    expect(resolveTarget("not-exist").section).toBe("general");
    expect(resolveTarget("privacy#nope").anchor).toBe("");
    expect(resolveTarget("general#shield").anchor).toBe("");
  });
});

describe("searchSettings", () => {
  it("空查询返回空数组", () => {
    expect(searchSettings("")).toEqual([]);
    expect(searchSettings("   ")).toEqual([]);
  });

  it("中文/英文/别名均可命中且目标正确", () => {
    expect(searchSettings("mcp").some((r) => r.section === "ai" && r.aiTab === "mcp")).toBe(true);
    expect(searchSettings("cookie")[0]).toMatchObject({
      section: "privacy",
      anchor: "history-cleanup",
    });
    expect(searchSettings("只读")[0]).toMatchObject({ section: "advanced", anchor: "api" });
    expect(searchSettings("壁纸")[0]).toMatchObject({
      section: "appearance",
      anchor: "newtab",
    });
    expect(searchSettings("油猴")[0]).toMatchObject({
      section: "extensions",
      anchor: "userscripts",
    });
    expect(searchSettings("dmabuf")[0]).toMatchObject({
      section: "advanced",
      anchor: "compat",
    });
  });

  it("大小写不敏感且结果数上限 12", () => {
    expect(searchSettings("MCP").length).toBeGreaterThan(0);
    expect(searchSettings("a").length).toBeLessThanOrEqual(12);
  });
});

describe("SETTING_SEARCH_INDEX 注册表防漂移", () => {
  it("每条目栏目合法、锚点在该栏白名单内", () => {
    for (const e of SETTING_SEARCH_INDEX) {
      expect(SECTION_IDS).toContain(e.section);
      if (e.anchor) expect(SECTION_ANCHORS[e.section]).toContain(e.anchor);
      if (e.section === "ai") expect(e.aiTab).toBeTruthy();
      if (e.section !== "ai") expect(e.aiTab).toBeUndefined();
    }
  });

  it("非 AI 锚点必须真实存在于 settings/*.vue 的 data-anchor", () => {
    const used = new Set(
      SETTING_SEARCH_INDEX.filter((e) => e.section !== "ai" && e.anchor).map((e) => e.anchor),
    );
    for (const a of used) {
      expect(vueAnchors.has(a), `SFC 中找不到 data-anchor="${a}"`).toBe(true);
    }
  });

  it("SECTION_ANCHORS 白名单中的非 AI 锚点与 SFC 双向一致（防漏/防改名）", () => {
    for (const id of SECTION_IDS) {
      if (id === "ai") continue;
      for (const a of SECTION_ANCHORS[id as SectionId]) {
        expect(vueAnchors.has(a), `白名单锚点 ${id}#${a} 在 SFC 中不存在`).toBe(true);
      }
    }
  });

  it("AI 三个页签 id 在 AiSection.vue 中存在", () => {
    for (const t of ["model", "mcp", "labs"]) {
      expect(aiSectionSrc).toContain(`'${t}'`);
    }
  });
});
