(() => {
  // Load GLightbox from CDN
  const loadGLightbox = () =>
    new Promise((resolve, reject) => {
      if (window.GLightbox) {
        resolve(window.GLightbox);
        return;
      }
      const link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = 'https://cdn.jsdelivr.net/npm/glightbox@3.3.0/dist/css/glightbox.min.css';
      document.head.appendChild(link);

      const script = document.createElement('script');
      script.src = 'https://cdn.jsdelivr.net/npm/glightbox@3.3.0/dist/js/glightbox.min.js';
      script.async = true;
      script.onload = () => resolve(window.GLightbox);
      script.onerror = reject;
      document.head.appendChild(script);
    });

  const initLightbox = async () => {
    const GLightbox = await loadGLightbox();
    if (!GLightbox) return;

    // Collect all images in content (skip Excalidraw diagrams which have built-in zoom)
    const elements = [];
    document.querySelectorAll('.page-content img').forEach((img) => {
      // Skip images inside excalidraw elements
      if (img.closest('.excalidraw-container')) return;

      elements.push({
        href: img.src,
        type: 'image',
        title: img.alt || img.title || '',
        description: img.alt || '',
      });
    });

    if (elements.length > 0) {
      GLightbox({
        elements,
        zoomable: true,
        draggable: true,
      });
    }
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initLightbox);
  } else {
    initLightbox();
  }
})();
