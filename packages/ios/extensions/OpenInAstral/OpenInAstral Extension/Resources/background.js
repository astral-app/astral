browser.runtime.onMessage.addListener(message => {
    if (!message || typeof message.browserLoginStage !== 'string') return;
    browser.runtime.sendNativeMessage({ browserLoginStage: message.browserLoginStage }).catch(() => {});
});
