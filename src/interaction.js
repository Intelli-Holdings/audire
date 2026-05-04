// Shared interaction primitives for the framework-light UI.
// Kept small on purpose: autosizing textareas, Enter submit, and dialog focus.

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'textarea:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function getVerticalPadding(el) {
  const style = getComputedStyle(el);
  return (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
}

function getLineHeight(el) {
  const style = getComputedStyle(el);
  const parsed = parseFloat(style.lineHeight);
  if (Number.isFinite(parsed)) return parsed;
  return (parseFloat(style.fontSize) || 14) * 1.5;
}

function rowsForMode(mode, fallback) {
  if (mode === 'expanded') return Math.max(fallback, 6);
  if (mode === 'comfortable') return Math.max(fallback, 3);
  return fallback;
}

export function setupAutosizeTextarea(textarea, options = {}) {
  if (!textarea) return () => {};

  const {
    minRows = Number(textarea.getAttribute('rows')) || 1,
    maxVh = 0.36,
    sizeMode = textarea.dataset.sizeMode || 'compact',
    storageKey = '',
  } = options;

  const storedMode = storageKey ? localStorageSafeGet(storageKey) : '';
  textarea.dataset.sizeMode = storedMode || sizeMode;
  textarea.classList.add('autosize-textarea');

  let frame = 0;

  const resize = () => {
    frame = 0;
    const modeRows = rowsForMode(textarea.dataset.sizeMode, minRows);
    const lineHeight = getLineHeight(textarea);
    const padding = getVerticalPadding(textarea);
    const minHeight = Math.ceil(lineHeight * modeRows + padding);
    const maxHeight = Math.max(minHeight, Math.floor(window.innerHeight * maxVh));
    const previousScrollTop = textarea.scrollTop;
    const wasAtBottom = textarea.scrollTop + textarea.clientHeight >= textarea.scrollHeight - 2;

    textarea.style.height = 'auto';
    const nextHeight = Math.min(Math.max(textarea.scrollHeight, minHeight), maxHeight);
    textarea.style.minHeight = `${minHeight}px`;
    textarea.style.maxHeight = `${maxHeight}px`;
    textarea.style.height = `${nextHeight}px`;
    textarea.style.overflowY = textarea.scrollHeight > maxHeight ? 'auto' : 'hidden';

    if (wasAtBottom) {
      textarea.scrollTop = textarea.scrollHeight;
    } else {
      textarea.scrollTop = previousScrollTop;
    }
  };

  const schedule = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(resize);
  };

  const onModeChange = (event) => {
    const nextMode = event.detail?.mode;
    if (!nextMode) return;
    textarea.dataset.sizeMode = nextMode;
    if (storageKey) localStorageSafeSet(storageKey, nextMode);
    schedule();
  };

  textarea.addEventListener('input', schedule);
  textarea.addEventListener('paste', schedule);
  textarea.addEventListener('compositionend', schedule);
  textarea.addEventListener('audire:autosize', schedule);
  textarea.addEventListener('audire:autosize-mode', onModeChange);
  window.addEventListener('resize', schedule);
  schedule();

  return () => {
    if (frame) cancelAnimationFrame(frame);
    textarea.removeEventListener('input', schedule);
    textarea.removeEventListener('paste', schedule);
    textarea.removeEventListener('compositionend', schedule);
    textarea.removeEventListener('audire:autosize', schedule);
    textarea.removeEventListener('audire:autosize-mode', onModeChange);
    window.removeEventListener('resize', schedule);
  };
}

export function setTextareaValue(textarea, value, options = {}) {
  if (!textarea) return;
  const wasFocused = document.activeElement === textarea;
  const start = options.selectionStart ?? textarea.selectionStart;
  const end = options.selectionEnd ?? textarea.selectionEnd;

  textarea.value = value;
  textarea.dispatchEvent(new CustomEvent('audire:autosize'));

  if (wasFocused || options.focus) {
    textarea.focus();
    const nextStart = options.scrollToEnd ? textarea.value.length : start;
    const nextEnd = options.scrollToEnd ? textarea.value.length : end;
    try {
      textarea.setSelectionRange(nextStart, nextEnd);
    } catch { /* unsupported input type */ }
  }

  if (options.scrollToEnd) {
    requestAnimationFrame(() => {
      textarea.scrollTop = textarea.scrollHeight;
    });
  }
}

export function setAutosizeMode(textarea, mode) {
  if (!textarea) return;
  textarea.dispatchEvent(new CustomEvent('audire:autosize-mode', { detail: { mode } }));
}

export function bindTextareaSubmit(textarea, onSubmit) {
  if (!textarea) return () => {};
  let composing = false;

  const onCompositionStart = () => { composing = true; };
  const onCompositionEnd = () => { composing = false; };
  const onKeyDown = async (event) => {
    if (event.key !== 'Enter' || event.shiftKey || event.altKey || event.ctrlKey || event.metaKey) return;
    if (composing || event.isComposing) return;
    event.preventDefault();
    await onSubmit(event);
  };

  textarea.addEventListener('compositionstart', onCompositionStart);
  textarea.addEventListener('compositionend', onCompositionEnd);
  textarea.addEventListener('keydown', onKeyDown);

  return () => {
    textarea.removeEventListener('compositionstart', onCompositionStart);
    textarea.removeEventListener('compositionend', onCompositionEnd);
    textarea.removeEventListener('keydown', onKeyDown);
  };
}

export function setupTextareaSizeControls(container, textarea, options = {}) {
  if (!container || !textarea) return;
  const storageKey = options.storageKey || '';
  const current = textarea.dataset.sizeMode || localStorageSafeGet(storageKey) || 'compact';

  container.querySelectorAll('[data-textarea-size]').forEach((button) => {
    const mode = button.dataset.textareaSize;
    button.classList.toggle('active', mode === current);
    button.setAttribute('aria-pressed', String(mode === current));
    button.addEventListener('click', () => {
      setAutosizeMode(textarea, mode);
      container.querySelectorAll('[data-textarea-size]').forEach((item) => {
        const active = item.dataset.textareaSize === mode;
        item.classList.toggle('active', active);
        item.setAttribute('aria-pressed', String(active));
      });
      if (storageKey) localStorageSafeSet(storageKey, mode);
      textarea.focus();
    });
  });
}

export function trapDialogFocus(dialog, options = {}) {
  if (!dialog) return () => {};
  const restoreTo = options.restoreFocusTo || document.activeElement;

  const focusables = () => Array.from(dialog.querySelectorAll(FOCUSABLE_SELECTOR))
    .filter((el) => el.offsetParent !== null || el === document.activeElement);

  const onKeyDown = (event) => {
    if (event.key === 'Escape' && options.onEscape) {
      event.preventDefault();
      options.onEscape();
      return;
    }
    if (event.key !== 'Tab') return;

    const items = focusables();
    if (!items.length) {
      event.preventDefault();
      return;
    }

    const first = items[0];
    const last = items[items.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  dialog.addEventListener('keydown', onKeyDown);

  const target = options.initialFocus || focusables()[0] || dialog;
  requestAnimationFrame(() => target?.focus?.());

  return () => {
    dialog.removeEventListener('keydown', onKeyDown);
    if (options.restoreFocus !== false && restoreTo && typeof restoreTo.focus === 'function') {
      restoreTo.focus();
    }
  };
}

function localStorageSafeGet(key) {
  if (!key) return '';
  try { return localStorage.getItem(key) || ''; } catch { return ''; }
}

function localStorageSafeSet(key, value) {
  if (!key) return;
  try { localStorage.setItem(key, value); } catch { /* ignore */ }
}
