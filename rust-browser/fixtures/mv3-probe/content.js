chrome.runtime.sendMessage({kind: 'aurora-probe'}, response => {
  if (chrome.runtime.lastError) {
    document.title = 'AURORA_PROBE_ERROR';
    return;
  }
  if (response?.worker && response.cookiesDenied && response.id && response.visits > 0) {
    document.title = 'AURORA_MV3_OK:' + JSON.stringify(response);
  } else {
    document.title = 'AURORA_PROBE_ERROR';
  }
});
