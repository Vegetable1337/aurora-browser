chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (message.kind !== 'aurora-probe' || !sender.url?.startsWith('http://127.0.0.1:')) return;
  chrome.storage.local.get('visits').then(({visits = 0}) => {
    const count = visits + 1;
    return chrome.storage.local.set({visits: count}).then(() => respond({
      worker: true, visits: count, id: chrome.runtime.id,
      cookiesDenied: typeof chrome.cookies === 'undefined'
    }));
  }).catch(error => respond({error: String(error)}));
  return true;
});
