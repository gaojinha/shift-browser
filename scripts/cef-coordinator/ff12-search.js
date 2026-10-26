(() => {
  // F-CEF-12 真机验收·第 2/5 项：搜索过滤 + 空态文案（会改搜索框，跑完自动还原）
  // 判据源：toolbar.rs:423-433 —— q 小写后匹配 p.domain 或 p.username 的 includes；
  //         hit=0 时 .pw-empty 文案：pwList.length===0 → 「密码库为空」，否则 → 「没有匹配项」。
  //         触发口两条：pwSearch input(468) 与 keydown Escape 关面板(469-471)。
  const q = (s) => document.querySelector(s);
  const rows = () => Array.from(document.querySelectorAll('.pw-row'));
  const s = q('#pwsearch');
  const out = { ran: false };
  if (!s) { out.err = 'no #pwsearch'; return out; }

  const before = rows().length;
  const fire = (v) => {
    s.value = v;
    s.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const snap = () => ({
    n: rows().length,
    doms: rows().map((x) => (x.querySelector('.pw-dom') || {}).textContent),
    usrs: rows().map((x) => (x.querySelector('.pw-usr') || {}).textContent),
    empty: (q('.pw-empty') || {}).textContent || null,
  });

  // ① 用一个必然存在的站点片段搜（从当前 DOM 第一行的 .pw-dom 取，不写死域名，
  //    避免真库/隔离库两种数据都跑得出结论）
  const firstDom = before ? (rows()[0].querySelector('.pw-dom') || {}).textContent || '' : '';
  const key = firstDom.split('.')[0].toLowerCase();
  let a = null;
  if (key.length >= 3) { fire(key); a = snap(); }

  // ② 必然无匹配的关键词 → 应出现「没有匹配项」（库里有条目时）
  fire('zzz_no_such_site_9x7q');
  const b = snap();

  // ③ 用户名侧匹配（若第一行有用户名，取其前 3 字符）
  const firstUsr = before ? (rows()[0].querySelector('.pw-usr') || {}).textContent || '' : '';
  const uk = firstUsr.slice(0, 3).toLowerCase();
  let c = null;
  if (uk.length >= 2) { fire(uk); c = snap(); }

  // ④ 还原：清空搜索框 + 关面板，别污染后面的用例
  fire('');
  const d = snap();
  out.ran = true;
  out.before_n = before;
  out.key_domain = key || null;
  out.by_domain = a;
  out.by_none = { input: 'zzz_no_such_site_9x7q', ...b };
  out.key_username = uk || null;
  out.by_username = c;
  out.after_reset_n = d.n;
  // 判据：② 应为「没有匹配项」（不是「密码库为空」）；④ 应回到 before
  out.verdict_domain = a ? (a.n > 0 && a.n <= before && a.doms.every((x) => (x || '').toLowerCase().includes(key))) : null;
  out.verdict_none = b.empty === '没有匹配项' ? 'ok' : (before === 0 ? 'no-rows-to-test' : 'BAD:' + b.empty);
  out.verdict_reset = d.n === before;
  out.verdict_username = c ? (c.n > 0 && c.usrs.every((x) => (x || '').toLowerCase().includes(uk))) : null;
  return out;
})();
