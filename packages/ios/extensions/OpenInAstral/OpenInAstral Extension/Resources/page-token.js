(() => {
    if (window.top !== window) return;

    const nonce = new URL(document.currentScript.src).searchParams.get('nonce');
    if (!nonce) return;

    let reportedMissingToken = false;
    let reportedHandoffFailure = false;
    let handoffCreated = false;
    let handoffInFlight = false;
    let nextAttemptAt = 0;

    function sendStage(stage) {
        window.postMessage({ browserLoginStage: stage, browserLoginNonce: nonce }, window.location.origin);
    }

    function readToken() {
        let stored;
        try {
            stored = window.localStorage.getItem('token');
        } catch {
            return null;
        }
        if (!stored) return null;

        try {
            const token = JSON.parse(stored);
            return typeof token === 'string' && token.length >= 20 ? token : null;
        } catch {
            return typeof stored === 'string' && stored.length >= 20 ? stored : null;
        }
    }

    async function createHandoff() {
        if (handoffCreated || handoffInFlight || Date.now() < nextAttemptAt) return;
        const token = readToken();
        if (!token) {
            if (!reportedMissingToken) {
                reportedMissingToken = true;
                sendStage('token-unavailable');
            }
            return;
        }

        handoffInFlight = true;
        nextAttemptAt = Date.now() + 3000;
        const key = crypto.randomUUID();
        try {
            const response = await fetch('/api/v9/auth/handoff', {
                method: 'POST',
                credentials: 'same-origin',
                headers: {
                    Authorization: token,
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ key })
            });
            if (!response.ok) throw new Error('handoff request failed');
            const result = await response.json();
            if (typeof result.handoff_token !== 'string' || result.handoff_token.length < 20) throw new Error('handoff token unavailable');
            handoffCreated = true;
            window.postMessage({
                browserLoginHandoffToken: result.handoff_token,
                browserLoginHandoffKey: key,
                browserLoginNonce: nonce
            }, window.location.origin);
            sendStage('handoff-created');
        } catch {
            if (!reportedHandoffFailure) {
                reportedHandoffFailure = true;
                sendStage('handoff-failed');
            }
        } finally {
            handoffInFlight = false;
        }
    }

    sendStage('reader-running');
    createHandoff();
    setInterval(createHandoff, 500);
})();
