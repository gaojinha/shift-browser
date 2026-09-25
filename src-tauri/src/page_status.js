// read_status 固定状态采集脚本（编译期嵌入，include_str! 于 api/handlers/page.rs）
//
// 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（P0 · read_status）
// - 只取页面就绪态与结构提示，**不读取任何 DOM 值**（无脱敏面）；
// - 脚本自身不抛异常：任何错误返回 JSON.stringify({ error: ... })，由 Rust 侧转 ok:false。

(function () {
  function finish(v) { return JSON.stringify(v); }
  function fail(m) { return JSON.stringify({ error: m }); }
  try {
    var doc = document;
    var hints = [];
    var body = doc.body;
    if (!body) hints.push('页面无 body（可能仍在加载或非标准页面）');
    // 骨架屏 / 遮罩粗判：文档高度过小
    try {
      var h = doc.documentElement ? doc.documentElement.scrollHeight : 0;
      if (h > 0 && h < 200) hints.push('文档高度很小，可能是骨架屏或加载中');
    } catch (e) { /* ignore */ }
    var counts = {};
    try {
      counts.links = doc.links ? doc.links.length : 0;
      counts.images = doc.images ? doc.images.length : 0;
      counts.forms = doc.forms ? doc.forms.length : 0;
    } catch (e) { /* ignore */ }
    return finish({
      ready_state: doc.readyState || 'unknown',
      title: doc.title || '',
      url: location.href || '',
      has_body: !!body,
      hints: hints,
      counts: counts
    });
  } catch (e) {
    return fail('状态采集失败: ' + e.message);
  }
})();
