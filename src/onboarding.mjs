export const CHOICES = [
  { id: 'cortex', label: 'Cortex', description: 'Code graphs', includes: ['cortex'] },
  { id: 'membrane', label: 'Membrane (includes Cortex)', description: 'Context + memory — includes Cortex, auto-selected and non-deselectable', includes: ['cortex', 'membrane'] },
];

export function isValidChoice(id) {
  return CHOICES.some(c => c.id === id);
}

// Membrane selection implicitly includes cortex and cannot be deselected.
export function includesCortex(choiceId) {
  return true;
}
export function includesMembrane(choiceId) {
  return choiceId === 'membrane';
}

export function renderOnboarding(root, onSelect) {
  root.innerHTML = `<section class="onboarding" role="dialog" aria-label="Choose product"><h2>Choose your experience</h2><div class="choices">${CHOICES.map(c => `<button data-choice="${c.id}" class="choice"><strong>${c.label}</strong><span>${c.description}</span></button>`).join('')}</div></section>`;
  for (const btn of root.querySelectorAll('[data-choice]')) {
    btn.addEventListener('click', () => onSelect(btn.dataset.choice));
  }
}
