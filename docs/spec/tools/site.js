// 規格文件的少量互動：架構圖放大檢視、表格關鍵字篩選。沒有外部相依，離線可用。
(function () {
  var zoom = document.querySelector('.zoom');
  var zimg = zoom && zoom.querySelector('img');
  function close() { if (zoom) { zoom.hidden = true; zimg.removeAttribute('src'); } }
  document.addEventListener('click', function (e) {
    var a = e.target.closest && e.target.closest('a[data-zoom]');
    if (a && zoom) {
      e.preventDefault();
      zimg.src = a.getAttribute('href');
      zimg.alt = (a.querySelector('img') || {}).alt || '';
      zoom.hidden = false;
      return;
    }
    if (e.target.closest && e.target.closest('[data-zoom-close]')) close();
  });
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape') close(); });

  document.querySelectorAll('input[data-filter]').forEach(function (input) {
    var box = input.closest('.filter').nextElementSibling;
    var rows = box ? box.querySelectorAll('tbody tr') : [];
    input.addEventListener('input', function () {
      var q = input.value.trim().toLowerCase();
      rows.forEach(function (tr) {
        tr.style.display = !q || tr.textContent.toLowerCase().indexOf(q) >= 0 ? '' : 'none';
      });
    });
  });
})();
