async function check() {
  const dot = document.getElementById('dot');
  const status = document.getElementById('status');
  try {
    const r = await fetch('http://127.0.0.1:7777/health');
    const j = await r.json();
    if (j.ok) {
      dot.className = 'dot ok';
      status.textContent = `lilyd v${j.version} · model ${j.model} · uptime ${j.uptime_s}s`;
    } else {
      dot.className = 'dot bad';
      status.textContent = 'daemon responded but not ok';
    }
  } catch (e) {
    dot.className = 'dot bad';
    status.textContent = 'cannot reach lilyd on 127.0.0.1:7777 — is it running?';
  }
}

document.getElementById('reconnect').addEventListener('click', async () => {
  await chrome.runtime.sendMessage({ type: 'reconnect' }).catch(() => {});
  setTimeout(check, 500);
});

check();
setInterval(check, 5000);
