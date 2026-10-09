// La burbuja: un clic trae el panel, otro lo esconde.
const { invoke } = window.__TAURI__.core;

document.getElementById('burbuja').addEventListener('click', async () => {
  try {
    await invoke('burbuja_pulsada');
  } catch (e) {
    console.error('[burbuja]', e);
  }
});
