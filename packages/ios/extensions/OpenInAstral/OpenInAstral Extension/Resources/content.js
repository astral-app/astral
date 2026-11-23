const defaultSettings = {
    showBanner: true,
    instantOpen: false,
    urlScheme: 'astral',
    enableInvites: true,
    enableUsers: true,
    enableChannels: true,
    enableFavorites: true
};

let settings = { ...defaultSettings };
let browserLogin = null;
let browserLoginTokenReaderInjected = false;
let browserLoginTokenReaderNonce = null;
let browserLoginCallback = null;
let browserLoginStatus = null;
let browserLoginStatusAction = null;
let browserLoginStatusDismissed = false;

function reportBrowserLoginStage(stage) {
    browser.runtime.sendMessage({ browserLoginStage: stage }).catch(() => {});
}

function saveBrowserLogin() {
    if (browserLogin) {
        window.sessionStorage.setItem('browserLogin', JSON.stringify(browserLogin));
    }
}

function clearBrowserLogin() {
    window.sessionStorage.removeItem('browserLogin');
    browserLogin = null;
}

function restoreBrowserLogin() {
    const stored = window.sessionStorage.getItem('browserLogin');
    if (!stored) return;

    try {
        const session = JSON.parse(stored);
        if (typeof session.state === 'string' && typeof session.createdAt === 'number' && Date.now() - session.createdAt <= 600000) {
            browserLogin = session;
        }
    } catch {
        window.sessionStorage.removeItem('browserLogin');
    }
}

function expireBrowserLogin() {
    if (!browserLogin) return;
    const state = browserLogin.state;
    const delay = Math.max(0, 600000 - (Date.now() - browserLogin.createdAt));
    setTimeout(() => {
        if (browserLogin?.state !== state) return;
        clearBrowserLogin();
        browserLoginCallback = null;
        setBrowserLoginStatus('Sign-in timed out. Start passkey sign-in again.');
    }, delay);
}

function isDiscordPage() {
    return window.location.hostname === 'discord.com' || window.location.hostname.endsWith('.discord.com');
}

function isDiscordHost(hostname) {
    const host = hostname.toLowerCase();
    return host === 'discord.com' || host.endsWith('.discord.com') || host === 'discord.gg' || host.endsWith('.discord.gg');
}

function safeUrlScheme() {
    const scheme = typeof settings.urlScheme === 'string' ? settings.urlScheme.trim() : '';
    return scheme.length <= 128 && /^[a-z][a-z0-9+.-]*$/i.test(scheme) && !['javascript', 'data', 'file'].includes(scheme.toLowerCase())
        ? scheme
        : defaultSettings.urlScheme;
}

function injectBrowserLoginTokenReader() {
    if (!browserLogin || browserLoginTokenReaderInjected || !isDiscordPage()) return;

    browserLoginTokenReaderInjected = true;
    browserLoginTokenReaderNonce = crypto.randomUUID();
    const script = document.createElement('script');
    script.src = `${browser.runtime.getURL('page-token.js')}?nonce=${encodeURIComponent(browserLoginTokenReaderNonce)}`;
    script.addEventListener('load', () => {
        reportBrowserLoginStage('reader-loaded');
        script.remove();
    }, { once: true });
    script.addEventListener('error', () => {
        reportBrowserLoginStage('reader-failed');
        browserLoginTokenReaderInjected = false;
        browserLoginTokenReaderNonce = null;
        script.remove();
    }, { once: true });
    document.documentElement.appendChild(script);
}

function renderBrowserLoginStatus() {
    if (!browserLoginStatus || browserLoginStatusDismissed || !document.body) return;

    let panel = document.getElementById('browserLoginStatus');
    if (!panel) {
        panel = document.createElement('div');
        panel.id = 'browserLoginStatus';
        panel.style.background = '#111214';
        panel.style.border = '1px solid #5865f2';
        panel.style.borderRadius = '12px';
        panel.style.boxShadow = '0 8px 28px rgba(0, 0, 0, 0.35)';
        panel.style.color = '#ffffff';
        panel.style.fontFamily = '-apple-system, BlinkMacSystemFont, sans-serif';
        panel.style.fontSize = '15px';
        panel.style.left = '16px';
        panel.style.padding = '14px 16px';
        panel.style.position = 'fixed';
        panel.style.right = '16px';
        panel.style.textAlign = 'center';
        panel.style.top = '16px';
        panel.style.zIndex = '2147483647';
        document.body.appendChild(panel);
    }

    panel.textContent = '';
    const dismiss = document.createElement('button');
    dismiss.textContent = '×';
    dismiss.style.background = 'transparent';
    dismiss.style.border = '0';
    dismiss.style.color = '#b5bac1';
    dismiss.style.fontSize = '22px';
    dismiss.style.lineHeight = '1';
    dismiss.style.padding = '0';
    dismiss.style.position = 'absolute';
    dismiss.style.right = '10px';
    dismiss.style.top = '8px';
    dismiss.addEventListener('click', () => {
        browserLoginStatusDismissed = true;
        panel.remove();
    });
    panel.appendChild(dismiss);
    const message = document.createElement('div');
    message.textContent = browserLoginStatus;
    panel.appendChild(message);

    if (browserLoginStatusAction) {
        const button = document.createElement('button');
        button.textContent = 'RETURN TO ASTRAL';
        button.style.background = '#5865f2';
        button.style.border = '0';
        button.style.borderRadius = '6px';
        button.style.color = '#ffffff';
        button.style.fontSize = '13px';
        button.style.fontWeight = '700';
        button.style.marginTop = '10px';
        button.style.padding = '9px 14px';
        button.addEventListener('click', event => {
            event.preventDefault();
            browserLoginStatusAction();
        });
        panel.appendChild(button);
    }
}

function setBrowserLoginStatus(message, action = null) {
    browserLoginStatus = message;
    browserLoginStatusAction = action;
    renderBrowserLoginStatus();
}

function openBrowserLoginCallback() {
    if (!browserLoginCallback) return;
    window.location.assign(browserLoginCallback);
}

function handleBrowserLoginHandoff(token, key) {
    if (!browserLogin || typeof token !== 'string' || token.length < 20 || typeof key !== 'string' || key.length < 16 || browserLogin.returning) return;
    browserLogin.returning = true;
    setBrowserLoginStatus('Returning to astral…');
    const callback = new URL('astral://');
    callback.searchParams.set('handoff_token', token);
    callback.searchParams.set('handoff_key', key);
    callback.searchParams.set('redirect_to', 'astral://-/channels/@me');
    window.addEventListener('pagehide', clearBrowserLogin, { once: true });
    browserLoginCallback = callback.href;
    setBrowserLoginStatus('Returning to astral…', openBrowserLoginCallback);
    openBrowserLoginCallback();
    setTimeout(() => {
        if (browserLogin && browserLoginCallback) {
            browserLogin.returning = false;
            setBrowserLoginStatus('Login is ready. Tap below to return to astral.', openBrowserLoginCallback);
        }
    }, 1500);
}

window.addEventListener('message', event => {
    if (event.origin !== window.location.origin || !isDiscordPage()) return;

    const nonce = event.data?.browserLoginNonce;
    const stage = event.data?.browserLoginStage;
    if (nonce === browserLoginTokenReaderNonce && typeof stage === 'string') {
        reportBrowserLoginStage(stage);
        if (stage === 'handoff-failed') setBrowserLoginStatus('Could not prepare the astral login handoff. Retrying…');
    }
    const handoffToken = event.data?.browserLoginHandoffToken;
    const handoffKey = event.data?.browserLoginHandoffKey;
    if (nonce === browserLoginTokenReaderNonce && typeof handoffToken === 'string' && typeof handoffKey === 'string') {
        handleBrowserLoginHandoff(handoffToken, handoffKey);
    }
});

function captureBrowserLogin() {
    const fragment = new URLSearchParams(window.location.hash.slice(1));
    const state = fragment.get('astral-browser-login');
    if (!state || state.length > 128) return;

    browserLogin = { state, createdAt: Date.now(), returning: false };
    browserLoginStatusDismissed = false;
    reportBrowserLoginStage('handoff-captured');
    saveBrowserLogin();
    injectBrowserLoginTokenReader();
    setBrowserLoginStatus('Waiting for the Discord sign-in…');
    history.replaceState(null, '', `${window.location.pathname}${window.location.search}`);
}

captureBrowserLogin();

if (!browserLogin) {
    restoreBrowserLogin();
}

if (browserLogin) {
    expireBrowserLogin();
    setBrowserLoginStatus('Waiting for the Discord sign-in…');
    injectBrowserLoginTokenReader();
}

browser.storage.local.get(defaultSettings).then(stored => {
    settings = { ...defaultSettings, ...stored };
});

function transformUrl(url) {
    const urlObj = new URL(url);
    if (urlObj.protocol !== 'https:' || !isDiscordHost(urlObj.hostname)) return null;
    let path = urlObj.pathname;

    if (urlObj.hostname === 'discord.gg' || path.startsWith('/invite/')) {
        if (!settings.enableInvites) return null;
        const inviteCode = urlObj.hostname === 'discord.gg'
            ? path.split('/').filter(Boolean)[0]
            : path.split('/').filter(Boolean).pop();
        if (!inviteCode) return null;
        return `${safeUrlScheme()}://discord.com/invite/${encodeURIComponent(inviteCode)}`;
    }

    if (path.startsWith('/users/') && !settings.enableUsers) {
        return null;
    } else if (path.startsWith('/channels/') && !settings.enableChannels) {
        return null;
    } else if (path.startsWith('/favorites/') && !settings.enableFavorites) {
        return null;
    }

    return path ? `${safeUrlScheme()}://discord.com${path}${urlObj.search}${urlObj.hash}` : null;
}

const handledLinks = new WeakSet();

function handleLink(link) {
    if (!link || !link.href || handledLinks.has(link)) return;

    try {
        const url = new URL(link.href);
        if (isDiscordHost(url.hostname)) {
            const discordUrl = transformUrl(link.href);
            if (discordUrl) {
                handledLinks.add(link);
                link.addEventListener('click', event => {
                    event.preventDefault();
                    window.location.href = discordUrl;
                });
            }
        }
    } catch (error) {
        console.error('Invalid URL', link.href, error);
    }
}

function showBanner(discordUrl) {
    if (document.getElementById('astralBanner')) return;

    const meta = document.createElement('meta');
    meta.name = 'theme-color';
    meta.content = '#0f0e12';
    document.head.appendChild(meta);

    const banner = `
        <div id="astralBanner">
            <div class="brand-wrapper">
                <div class="brand-mark" aria-hidden="true">✦</div>
                <div class="text-wrapper">
                    <div class="title">astral</div>
                    <div class="subtitle">Open in the Discord app</div>
                </div>
            </div>
            <button class="openInAstralButton">OPEN</button>
        </div>
    `;

    if (!document.body) return;

    const insertBanner = () => {
        const existingBanner = document.getElementById('astralBanner');
        if (existingBanner) return;

        document.body.style.marginTop = '48px';
        document.body.insertAdjacentHTML('afterbegin', banner);

        document.querySelector('#astralBanner .openInAstralButton').addEventListener('click', event => {
            event.preventDefault();
            window.location.href = discordUrl;
        });
    };

    insertBanner();
}

function checkUrl() {
    if (!document.body) {
        setTimeout(checkUrl, 10);
        return;
    }

    renderBrowserLoginStatus();

    const url = window.location.href;
    const urlObj = new URL(url);

    if (isDiscordHost(urlObj.hostname) && !browserLogin) {
        const discordUrl = transformUrl(url);
        if (discordUrl) {
            if ((urlObj.hostname === 'discord.gg' || urlObj.pathname.startsWith('/invite/')) || settings.instantOpen) {
                window.location.href = discordUrl;
            }
            if (settings.showBanner) {
                showBanner(discordUrl);
            }
        }
    }
}

checkUrl();

let lastUrl = location.href;
new MutationObserver(() => {
    const url = location.href;
    if (url !== lastUrl) {
        lastUrl = url;
        checkUrl();
    }
}).observe(document, { subtree: true, childList: true });

document.onreadystatechange = function () {
    if (document.readyState === "complete") {
        document.querySelectorAll('a').forEach(handleLink);
    }
};

new MutationObserver(() => {
    if (document.readyState === "complete") {
        requestAnimationFrame(() => {
            document.querySelectorAll('a').forEach(handleLink);
        });
    }
}).observe(document.documentElement, {
    childList: true,
    subtree: true
});
