(() => {
  // F-CEF-12 真机验收·第 1/5 项（只读盘点）：列表条数 = 库里条数，且明文不进面板
  // 选择器全部 grep 自 src-tauri/src/engine/cef/toolbar.rs:413-493，不手抄：
  //   #pwbtn(235) #pwpanel(240) #pwsearch(241) #pwlist(242) #pw-data(245 是 <script type="application/json">)
  //   行 .pw-row[data-id](436-437) 掩码 .pw-mask='••••'(446-447) 按钮 .pw-btn[data-op=copy|del](449/453)
  //   空态 .pw-empty 文案「密码库为空」/「没有匹配项」(429-430) 开关态 html.pw-open(461)
  // 本脚本**只读**：不点任何按钮、不改搜索框。改状态的活交给 ff12-search/ff12-copy/ff12-del。
  const q = (s) => document.querySelector(s);
  const qa = (s) => Array.from(document.querySelectorAll(s));

  const r = {
    href: location.href,
    has_pwbtn: !!q('#pwbtn'),
    has_pwpanel: !!q('#pwpanel'),
    has_pwsearch: !!q('#pwsearch'),
    has_pwlist: !!q('#pwlist'),
    pw_open: document.documentElement.classList.contains('pw-open'),
    // 面板 top 受书签栏显隐影响（toolbar.rs:159 html:not(.bookmarks-hidden) #pwpanel{top:122px}）
    panel_top_px: q('#pwpanel') ? Math.round(q('#pwpanel').getBoundingClientRect().top) : null,
    bookmarks_hidden: document.documentElement.classList.contains('bookmarks-hidden'),
    win_h: window.innerHeight,
  };

  // ① 数据侧：pw-data 内嵌 JSON 的字段集合——安全红线要求只有 id/domain/username
  let data = null;
  const node = q('#pw-data');
  r.pw_data_present = !!node;
  if (node) {
    const raw = node.textContent || '';
    r.pw_data_raw_len = raw.length;
    try {
      data = JSON.parse(raw || '[]');
      r.pw_data_count = Array.isArray(data) ? data.length : null;
      const keys = new Set();
      (Array.isArray(data) ? data : []).forEach((p) => Object.keys(p || {}).forEach((k) => keys.add(k)));
      r.pw_data_keys = Array.from(keys).sort();
      // 疑似明文字段名（任一命中即 P0）
      const bad = ['password', 'pwd', 'secret', 'plain', 'pass'];
      r.pw_data_leak_keys = r.pw_data_keys.filter((k) => bad.some((b) => k.toLowerCase().includes(b)));
      // 值侧兜底：不该有 52~60 字符 base64 串或短明文出现在这些字段里
      const s = JSON.stringify(data);
      r.pw_data_has_b64_like = /[A-Za-z0-9+/]{48,}={0,2}/.test(s);
    } catch (e) {
      r.pw_data_parse_err = String(e).slice(0, 120);
    }
  }

  // ② DOM 侧：渲染出的行数、掩码、按钮对
  const rows = qa('.pw-row');
  r.dom_row_count = rows.length;
  r.dom_row_ids = rows.map((x) => x.dataset.id);
  r.all_have_id = rows.every((x) => !!x.dataset.id);
  r.mask_ok = rows.every((x) => {
    const m = x.querySelector('.pw-mask');
    return !!m && m.textContent === '••••';
  });
  r.copy_btn_count = qa('.pw-btn[data-op="copy"]').length;
  r.del_btn_count = qa('.pw-btn[data-op="del"]').length;
  r.armed_btn_count = qa('.pw-btn[data-op="del"][data-armed="1"]').length;
  const empty = q('.pw-empty');
  r.empty_text = empty ? empty.textContent : null;

  // ③ 整页 HTML 里找明文（最后一道：面板源码有没有漏）
  const html = document.documentElement.outerHTML;
  r.page_html_len = html.length;
  r.page_has_password_kw = /password["'\s:=]/i.test(html.replace(/#pwpanel|#pwbtn|#pwsearch|#pwlist|pw-row|pw-btn|pw-mask|pw-dom|pw-usr|pw-empty|pw-data|pw-open|pwList|setPwPanel|renderPasswords/g, ''));

  // ④ 对账判据（脚本不做结论，只把两边数摆出来，verify.sh 里比）
  r.match_db_to_dom = r.pw_data_count === null ? null : (r.pw_data_count === r.dom_row_count);
  r.domains_sample = (Array.isArray(data) ? data : []).slice(0, 5).map((p) => p.domain);
  return r;
})();
