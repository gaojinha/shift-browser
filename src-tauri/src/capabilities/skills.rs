//! `skills/list` API 响应（R1 拆分 · 2026-09-25）
//!
//! 技能数据源：`contracts::skills`，与前端生成物同源。

pub fn skills_list_json() -> serde_json::Value {
    serde_json::json!({
      "skills": crate::contracts::skills::SKILLS.iter().map(|s| serde_json::json!({
          "id": s.id, "name": s.name, "icon": s.icon,
          "desc": s.desc, "uses": s.uses,
      })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use crate::contracts::skills;

    #[test]
    fn skills_all_matches_skills() {
        // 生成用 ALL 与 SKILLS 必须一致（防两边漂移）
        assert_eq!(skills::SKILLS.len(), skills::ALL.len());
        for (s, (id, name, icon, _)) in skills::SKILLS.iter().zip(skills::ALL.iter()) {
            assert_eq!(s.id, *id);
            assert_eq!(s.name, *name);
            assert_eq!(s.icon, *icon);
        }
    }

    #[test]
    fn skills_list_json_shape() {
        // skills/list API 响应形状：键名/条数/字段透传，漂移即红（外部 MCP 消费方按此取数）
        let v = super::skills_list_json();
        let arr = v["skills"].as_array().expect("skills 应为数组");
        assert_eq!(arr.len(), skills::SKILLS.len());
        for (item, s) in arr.iter().zip(skills::SKILLS.iter()) {
            assert_eq!(item["id"], s.id);
            assert_eq!(item["name"], s.name);
            assert_eq!(item["icon"], s.icon);
            assert_eq!(item["desc"], s.desc);
            assert_eq!(item["uses"], serde_json::json!(s.uses));
        }
    }
}
