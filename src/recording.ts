import { listen } from '@tauri-apps/api/event';
import './recording.css';

const root = document.querySelector('#recording-app')!;

function render(label: string) {
  const text = label.replace(/…/g, '').toUpperCase();
  const state =
    text === 'ERROR'
      ? 'error'
      : text === 'CANCELLED'
        ? 'cancelled'
      : text === 'TRANSCRIBING'
        ? 'processing'
        : 'listening';
  root.innerHTML = `
    <section class="recording-pill ${state}" aria-live="polite">
      <div class="glitch-bars" aria-hidden="true">
        <span></span><span></span><span></span><span></span>
        <span></span><span></span><span></span><span></span>
      </div>
      <strong class="glitch-text">${text}</strong>
    </section>
  `;
}

render('Listening…');

listen('parrot:recording-started', () => render('Listening…'));
listen('parrot:recording-processing', () => render('Transcribing…'));
listen('parrot:recording-failed', () => render('Error'));
listen('parrot:recording-cancelled', () => render('Cancelled'));
