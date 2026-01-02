(() => {
  'use strict';

  let fullwindowDiv;
  let scrollHandler;

  window.windowfull = {
    full(element) {
      fullwindowDiv = document.createElement('div');
      const img = document.createElement('img');
      img.src = element.src;
      img.alt = element.alt;

      fullwindowDiv.appendChild(img);
      fullwindowDiv.classList.add('fullscreen');
      document.body.appendChild(fullwindowDiv);

      fullwindowDiv.addEventListener('click', () => {
        windowfull.exit();
      }, { once: true });

      scrollHandler = () => windowfull.exit();
      window.addEventListener('scroll', scrollHandler, { once: true });
    },

    exit() {
      if (!fullwindowDiv) return;
      fullwindowDiv.remove();
      fullwindowDiv = undefined;
      window.removeEventListener('scroll', scrollHandler);
    },

    toggle(element) {
      return fullwindowDiv ? this.exit() : this.full(element);
    }
  };
})();

document.querySelectorAll('img').forEach((img) => {
  img.addEventListener('click', function () {
    windowfull.toggle(this);
  });
});
