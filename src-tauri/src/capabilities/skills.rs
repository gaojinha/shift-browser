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
}
