(() => {
  const bm = document.getElementById('bookmarks');
  return {
    href: location.href,
    bookmarks_offsetHeight: bm ? bm.offsetHeight : 'NO #bookmarks 元素',
    bookmarks_display: bm ? getComputedStyle(bm).display : null,
    html_class: document.documentElement.className || '(空)',
    innerHeight: window.innerHeight,
    innerWidth: window.innerWidth,
    doc_height_css: getComputedStyle(document.documentElement).height,
    bookmark_count: bm ? bm.querySelectorAll('a, .bm, [data-bookmark]').length : null,
    判读: '展开应 offsetHeight=34/class 空/innerHeight=738；收起应 0/bookmarks-hidden/772'
  };
})()
