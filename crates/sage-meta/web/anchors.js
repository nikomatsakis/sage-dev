(() => {
  'use strict';
  const script = document.currentScript;
  const index = script.previousElementSibling;
  if (!index?.classList.contains('anchor-index')) return;
  const collapsed = new Set();
  const expandedText = new Set();
  const inside = (name, prefix) => name.startsWith(`${prefix}.`);
  const obscured = name => [...collapsed].some(prefix => inside(name, prefix));
  function update() {
    for (const row of index.querySelectorAll('[data-group]')) row.hidden = obscured(row.dataset.group);
    for (const row of index.querySelectorAll('[data-anchor]')) row.hidden = obscured(row.dataset.anchor);
    for (const row of index.querySelectorAll('[data-anchor-text]')) row.hidden = obscured(row.dataset.anchorText) || !expandedText.has(row.id);
    for (const button of index.querySelectorAll('[data-toggle-group]')) {
      const open = !collapsed.has(button.dataset.toggleGroup);
      button.setAttribute('aria-expanded', String(open));
      button.setAttribute('aria-label', `${open ? 'Collapse' : 'Expand'} ${button.dataset.toggleGroup}`);
      button.textContent = open ? '▾' : '▸';
    }
  }
  index.addEventListener('click', event => {
    const button = event.target.closest('button');
    if (!button) return;
    if (button.hasAttribute('data-toggle-group')) {
      const group = button.dataset.toggleGroup;
      if (collapsed.has(group)) collapsed.delete(group); else collapsed.add(group);
    } else if (button.hasAttribute('data-toggle-text')) {
      const id = button.dataset.toggleText;
      if (expandedText.has(id)) expandedText.delete(id); else expandedText.add(id);
      const open = expandedText.has(id);
      button.setAttribute('aria-expanded', String(open));
      button.textContent = open ? '▾' : '▸';
    } else if (button.hasAttribute('data-expand-groups')) {
      collapsed.clear();
    } else if (button.hasAttribute('data-collapse-groups')) {
      for (const group of index.querySelectorAll('[data-group]')) collapsed.add(group.dataset.group);
    }
    update();
  });
})();
