(() => {
  'use strict';

  let fullwindowDiv;
  let scrollHandler;

  window.windowfull = {
    full(imgElement) {
      fullwindowDiv = document.createElement('div');
      fullwindowDiv.classList.add('fullscreen');

      const figure = imgElement.closest('figure');
      if (figure) {
        // Clone the entire figure (img + figcaption)
        const clonedFigure = figure.cloneNode(true);
        fullwindowDiv.appendChild(clonedFigure);
      } else {
        const clonedImg = imgElement.cloneNode(true);
        fullwindowDiv.appendChild(clonedImg);
      }

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
