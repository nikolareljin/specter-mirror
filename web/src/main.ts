import './style.css';
import mirrorArtwork from '../../assets/illustrations/antique-mirror.svg';

const mirror = document.querySelector<HTMLImageElement>('.mirror img');
if (mirror) mirror.src = mirrorArtwork;

const reduced = document.querySelector<HTMLInputElement>('#reduced-motion');
if (reduced) {
  reduced.checked = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  document.documentElement.classList.toggle('reduced-motion', reduced.checked);
  reduced.addEventListener('change', () => {
    document.documentElement.classList.toggle('reduced-motion', reduced.checked);
  });
}
