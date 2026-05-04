// Toast notification system

export function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container')
    || (() => {
      const el = document.createElement('div');
      el.id = 'toast-container';
      document.body.appendChild(el);
      return el;
    })();

  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;
  toast.setAttribute('role', type === 'error' ? 'alert' : 'status');
  toast.setAttribute('aria-live', type === 'error' ? 'assertive' : 'polite');
  toast.textContent = message;
  container.appendChild(toast);

  const dismiss = () => {
    toast.classList.remove('toast-visible');
    setTimeout(() => toast.remove(), 250);
  };

  toast.addEventListener('click', dismiss);
  requestAnimationFrame(() => toast.classList.add('toast-visible'));
  setTimeout(dismiss, 3000);
}
