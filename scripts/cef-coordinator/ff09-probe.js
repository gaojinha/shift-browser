(() => {
  const bm = document.getElementById('bookmarks');
  return {
    href: location.href,
    bookmarks_offsetHeight: bm ? bm.offsetHeight : 'NO #bookmarks 元素',
    bookmarks_display: bm ? getComputedStyle(bm).display : null,
    html_class: document.documentElement.className || '(空)',
    innerHeight: window.innerHeight,
    innerWidth: window.innerWidth,
    outerHeight: window.outerHeight,
    // ★ 键盘判据（2026-10-26 13:05 c91d383 提出：View request_focus ≠ 网页能收键盘，
    //   CEF 的 browser 还要单独 set_focus(1)，那时 document.hasFocus() 一直 false）。
    //   Ctrl+Shift+B 挂在 CEF KeyboardHandler::on_pre_key_event（shell.rs:1342/1352），
    //   所以"键按下去没反应"要先分清：是 toggle_bookmarks 坏了，还是事件根本没进 browser。
    has_focus: (() => { try { return document.hasFocus(); } catch (e) { return 'ERR'; } })(),
    doc_height_css: getComputedStyle(document.documentElement).height,
    bookmark_count: bm ? bm.querySelectorAll('a, .bm, [data-bookmark]').length : null,
    // 判读口径以**本机实测**为准（12:52 三段复现 + 13:15 几何核对），不是抄来的数：
    //   工具栏页 innerHeight：展开 122 / 收起 88（书签栏 34px）
    //   #bookmarks.offsetHeight：展开 34 / 收起 0；html.class：'' / 'bookmarks-hidden'
    //   内容页 innerHeight：窗高 − 122（展开）/ 窗高 − 88（收起）。本机 897 窗时 = 775 / 809
    //   ⚠️ 别拿 738→772 当判据，那是另一台实例的尺寸（copilot 13:00 回执用的参照）。
    判读: '展开 offsetHeight=34 且工具栏 innerHeight=122；收起 0 且 88、class=bookmarks-hidden；内容页恒等于 窗高−工具栏高'
  };
})()
