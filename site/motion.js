// Revelação ao rolar e fio sob o cabeçalho. O conteúdo é visível por padrão:
// só depois que este script roda o <html> ganha .js-reveal e o que está abaixo
// da dobra passa a entrar ao rolar. O que já está na tela não pisca (.seen).
(function () {
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var top = document.querySelector('.top');
  if (top) {
    var sync = function () { top.classList.toggle('scrolled', window.scrollY > 8); };
    sync();
    window.addEventListener('scroll', sync, { passive: true });
  }

  if (reduced || !('IntersectionObserver' in window)) return;
  var vh = window.innerHeight || document.documentElement.clientHeight;
  var pending = [];
  document.querySelectorAll('.reveal').forEach(function (el) {
    var r = el.getBoundingClientRect();
    if (r.top < vh && r.bottom > 0) el.classList.add('seen'); else pending.push(el);
  });
  document.documentElement.classList.add('js-reveal');
  var io = new IntersectionObserver(function (entries, obs) {
    entries.forEach(function (entry, i) {
      if (!entry.isIntersecting) return;
      entry.target.style.animationDelay = Math.min(i, 4) * 65 + 'ms';
      entry.target.classList.add('visible');
      obs.unobserve(entry.target);
    });
  }, { threshold: 0.1, rootMargin: '0px 0px -40px 0px' });
  pending.forEach(function (el) { io.observe(el); });
})();
