/**
 * Channels management page JavaScript
 */

// API base URL
const API_BASE = '/api';

// Gateway WebSocket URL - uses same hostname, port 8081
// Can be overridden by setting window.GATEWAY_URL before this script loads
const GATEWAY_URL = window.GATEWAY_URL || `ws://${window.location.hostname}:8081`;

// Gateway client instance
let gateway = null;

// Activity log (max 50 entries)
const activityLog = [];
const MAX_ACTIVITY = 50;

/**
 * Initialize the page
 */
async function init() {
    // Check authentication
    const token = localStorage.getItem('stark_token');
    if (!token) {
        window.location.href = '/';
        return;
    }

    // Validate session
    try {
        const response = await fetch(`${API_BASE}/auth/validate`, {
            headers: { 'Authorization': `Bearer ${token}` }
        });
        const data = await response.json();
        if (!data.valid) {
            localStorage.removeItem('stark_token');
            window.location.href = '/';
            return;
        }
    } catch (error) {
        console.error('Session validation error:', error);
        localStorage.removeItem('stark_token');
        window.location.href = '/';
        return;
    }

    // Setup event listeners
    setupEventListeners();

    // Connect to Gateway
    connectToGateway();

    // Load channels
    loadChannels();
}

/**
 * Setup event listeners
 */
function setupEventListeners() {
    // Logout button
    document.getElementById('logout-btn').addEventListener('click', async () => {
        const token = localStorage.getItem('stark_token');
        try {
            await fetch(`${API_BASE}/auth/logout`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ token })
            });
        } catch (error) {
            console.error('Logout error:', error);
        }
        localStorage.removeItem('stark_token');
        window.location.href = '/';
    });

    // Channel type change - show/hide app token field
    document.getElementById('channel-type').addEventListener('change', (e) => {
        const appTokenField = document.getElementById('app-token-field');
        if (e.target.value === 'slack') {
            appTokenField.classList.remove('hidden');
        } else {
            appTokenField.classList.add('hidden');
        }
    });

    // Add channel form
    document.getElementById('add-channel-form').addEventListener('submit', async (e) => {
        e.preventDefault();
        await addChannel();
    });
}

/**
 * Connect to Gateway WebSocket
 */
async function connectToGateway() {
    gateway = new GatewayClient(GATEWAY_URL);
    console.log('Connecting to Gateway at:', GATEWAY_URL);

    gateway.onConnectionChange = (connected) => {
        updateGatewayStatus(connected);
    };

    // Subscribe to events
    gateway.on('channel.started', (data) => {
        addActivity(`Channel "${data.name}" (${data.channel_type}) started`);
        loadChannels();
    });

    gateway.on('channel.stopped', (data) => {
        addActivity(`Channel "${data.name}" (${data.channel_type}) stopped`);
        loadChannels();
    });

    gateway.on('channel.error', (data) => {
        addActivity(`Channel error: ${data.error}`, 'error');
        loadChannels();
    });

    gateway.on('channel.message', (data) => {
        addActivity(`Message from ${data.from} on ${data.channel_type}: "${truncate(data.text, 50)}"`);
    });

    gateway.on('agent.response', (data) => {
        addActivity(`Response to ${data.to}: "${truncate(data.text, 50)}"`);
    });

    try {
        await gateway.connect();
        updateGatewayStatus(true);
    } catch (error) {
        console.error('Failed to connect to Gateway:', error);
        updateGatewayStatus(false);
    }
}

/**
 * Update Gateway connection status display
 */
function updateGatewayStatus(connected) {
    const indicator = document.getElementById('gateway-status-indicator');
    const text = document.getElementById('gateway-status-text');

    if (connected) {
        indicator.classList.remove('bg-yellow-500', 'bg-red-500');
        indicator.classList.add('bg-green-500');
        text.textContent = 'Connected';
        text.classList.remove('text-slate-400', 'text-red-400');
        text.classList.add('text-green-400');
    } else {
        indicator.classList.remove('bg-green-500', 'bg-yellow-500');
        indicator.classList.add('bg-red-500');
        text.textContent = 'Disconnected';
        text.classList.remove('text-green-400', 'text-slate-400');
        text.classList.add('text-red-400');
    }
}

/**
 * Load channels from API
 */
async function loadChannels() {
    const token = localStorage.getItem('stark_token');
    const loading = document.getElementById('loading');
    const channelsList = document.getElementById('channels-list');
    const noChannels = document.getElementById('no-channels');

    loading.classList.remove('hidden');
    channelsList.classList.add('hidden');
    noChannels.classList.add('hidden');

    try {
        const response = await fetch(`${API_BASE}/channels`, {
            headers: { 'Authorization': `Bearer ${token}` }
        });
        const data = await response.json();

        loading.classList.add('hidden');

        if (data.success && data.channels && data.channels.length > 0) {
            channelsList.classList.remove('hidden');
            renderChannels(data.channels);
        } else {
            noChannels.classList.remove('hidden');
        }
    } catch (error) {
        console.error('Failed to load channels:', error);
        loading.classList.add('hidden');
        showError('Failed to load channels');
    }
}

/**
 * Render channels list
 */
function renderChannels(channels) {
    const container = document.getElementById('channels-list');
    container.innerHTML = channels.map(channel => `
        <div class="flex items-center justify-between p-4 bg-slate-900 rounded-lg">
            <div class="flex items-center gap-4">
                <div class="w-10 h-10 rounded-lg flex items-center justify-center ${channel.channel_type === 'telegram' ? 'bg-blue-500/20 text-blue-400' : 'bg-purple-500/20 text-purple-400'}">
                    ${getChannelIcon(channel.channel_type)}
                </div>
                <div>
                    <div class="font-medium text-white">${escapeHtml(channel.name)}</div>
                    <div class="text-sm text-slate-400">${capitalize(channel.channel_type)}</div>
                </div>
            </div>
            <div class="flex items-center gap-3">
                <div class="flex items-center gap-2">
                    <div class="w-2 h-2 rounded-full ${channel.running ? 'bg-green-500' : 'bg-slate-500'}"></div>
                    <span class="text-sm ${channel.running ? 'text-green-400' : 'text-slate-400'}">${channel.running ? 'Running' : 'Stopped'}</span>
                </div>
                ${channel.running ? `
                    <button onclick="stopChannel(${channel.id})" class="px-3 py-1.5 bg-red-500/20 hover:bg-red-500/30 text-red-400 rounded-lg text-sm font-medium transition-colors">
                        Stop
                    </button>
                ` : `
                    <button onclick="startChannel(${channel.id})" class="px-3 py-1.5 bg-green-500/20 hover:bg-green-500/30 text-green-400 rounded-lg text-sm font-medium transition-colors">
                        Start
                    </button>
                `}
                <button onclick="deleteChannel(${channel.id}, '${escapeHtml(channel.name)}')" class="px-3 py-1.5 bg-slate-700 hover:bg-slate-600 text-slate-300 rounded-lg text-sm font-medium transition-colors">
                    Delete
                </button>
            </div>
        </div>
    `).join('');
}

/**
 * Get channel type icon SVG
 */
function getChannelIcon(type) {
    if (type === 'telegram') {
        return `<svg class="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
            <path d="M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z"/>
        </svg>`;
    } else {
        return `<svg class="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
            <path d="M5.042 15.165a2.528 2.528 0 0 1-2.52 2.523A2.528 2.528 0 0 1 0 15.165a2.527 2.527 0 0 1 2.522-2.52h2.52v2.52zM6.313 15.165a2.527 2.527 0 0 1 2.521-2.52 2.527 2.527 0 0 1 2.521 2.52v6.313A2.528 2.528 0 0 1 8.834 24a2.528 2.528 0 0 1-2.521-2.522v-6.313zM8.834 5.042a2.528 2.528 0 0 1-2.521-2.52A2.528 2.528 0 0 1 8.834 0a2.528 2.528 0 0 1 2.521 2.522v2.52H8.834zM8.834 6.313a2.528 2.528 0 0 1 2.521 2.521 2.528 2.528 0 0 1-2.521 2.521H2.522A2.528 2.528 0 0 1 0 8.834a2.528 2.528 0 0 1 2.522-2.521h6.312zM18.956 8.834a2.528 2.528 0 0 1 2.522-2.521A2.528 2.528 0 0 1 24 8.834a2.528 2.528 0 0 1-2.522 2.521h-2.522V8.834zM17.688 8.834a2.528 2.528 0 0 1-2.523 2.521 2.527 2.527 0 0 1-2.52-2.521V2.522A2.527 2.527 0 0 1 15.165 0a2.528 2.528 0 0 1 2.523 2.522v6.312zM15.165 18.956a2.528 2.528 0 0 1 2.523 2.522A2.528 2.528 0 0 1 15.165 24a2.527 2.527 0 0 1-2.52-2.522v-2.522h2.52zM15.165 17.688a2.527 2.527 0 0 1-2.52-2.523 2.526 2.526 0 0 1 2.52-2.52h6.313A2.527 2.527 0 0 1 24 15.165a2.528 2.528 0 0 1-2.522 2.523h-6.313z"/>
        </svg>`;
    }
}

/**
 * Add a new channel
 */
async function addChannel() {
    const token = localStorage.getItem('stark_token');
    const channelType = document.getElementById('channel-type').value;
    const name = document.getElementById('channel-name').value.trim();
    const botToken = document.getElementById('bot-token').value.trim();
    const appToken = document.getElementById('app-token').value.trim();

    if (!name) {
        showError('Please enter a channel name');
        return;
    }

    if (!botToken) {
        showError('Please enter a bot token');
        return;
    }

    if (channelType === 'slack' && !appToken) {
        showError('Slack channels require an App Token for Socket Mode');
        return;
    }

    try {
        const body = {
            channel_type: channelType,
            name: name,
            bot_token: botToken
        };

        if (channelType === 'slack') {
            body.app_token = appToken;
        }

        const response = await fetch(`${API_BASE}/channels`, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${token}`,
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(body)
        });

        const data = await response.json();

        if (data.success) {
            showSuccess('Channel created successfully');
            document.getElementById('add-channel-form').reset();
            document.getElementById('app-token-field').classList.add('hidden');
            loadChannels();
        } else {
            showError(data.error || 'Failed to create channel');
        }
    } catch (error) {
        console.error('Failed to create channel:', error);
        showError('Failed to create channel');
    }
}

/**
 * Start a channel
 */
async function startChannel(id) {
    const token = localStorage.getItem('stark_token');

    try {
        const response = await fetch(`${API_BASE}/channels/${id}/start`, {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` }
        });

        const data = await response.json();

        if (data.success) {
            showSuccess('Channel started');
            loadChannels();
        } else {
            showError(data.error || 'Failed to start channel');
        }
    } catch (error) {
        console.error('Failed to start channel:', error);
        showError('Failed to start channel');
    }
}

/**
 * Stop a channel
 */
async function stopChannel(id) {
    const token = localStorage.getItem('stark_token');

    try {
        const response = await fetch(`${API_BASE}/channels/${id}/stop`, {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` }
        });

        const data = await response.json();

        if (data.success) {
            showSuccess('Channel stopped');
            loadChannels();
        } else {
            showError(data.error || 'Failed to stop channel');
        }
    } catch (error) {
        console.error('Failed to stop channel:', error);
        showError('Failed to stop channel');
    }
}

/**
 * Delete a channel
 */
async function deleteChannel(id, name) {
    if (!confirm(`Are you sure you want to delete channel "${name}"?`)) {
        return;
    }

    const token = localStorage.getItem('stark_token');

    try {
        const response = await fetch(`${API_BASE}/channels/${id}`, {
            method: 'DELETE',
            headers: { 'Authorization': `Bearer ${token}` }
        });

        const data = await response.json();

        if (data.success) {
            showSuccess('Channel deleted');
            loadChannels();
        } else {
            showError(data.error || 'Failed to delete channel');
        }
    } catch (error) {
        console.error('Failed to delete channel:', error);
        showError('Failed to delete channel');
    }
}

/**
 * Add an activity entry
 */
function addActivity(message, type = 'info') {
    const timestamp = new Date().toLocaleTimeString();
    activityLog.unshift({ timestamp, message, type });

    if (activityLog.length > MAX_ACTIVITY) {
        activityLog.pop();
    }

    renderActivity();
}

/**
 * Render activity log
 */
function renderActivity() {
    const container = document.getElementById('activity-list');

    if (activityLog.length === 0) {
        container.innerHTML = '<p class="text-slate-500 text-sm">No recent activity</p>';
        return;
    }

    container.innerHTML = activityLog.map(entry => `
        <div class="flex gap-2 text-sm">
            <span class="text-slate-500 flex-shrink-0">${entry.timestamp}</span>
            <span class="${entry.type === 'error' ? 'text-red-400' : 'text-slate-300'}">${escapeHtml(entry.message)}</span>
        </div>
    `).join('');
}

/**
 * Show success message
 */
function showSuccess(message) {
    const el = document.getElementById('success-message');
    el.textContent = message;
    el.classList.remove('hidden');
    document.getElementById('error-message').classList.add('hidden');

    setTimeout(() => {
        el.classList.add('hidden');
    }, 5000);
}

/**
 * Show error message
 */
function showError(message) {
    const el = document.getElementById('error-message');
    el.textContent = message;
    el.classList.remove('hidden');
    document.getElementById('success-message').classList.add('hidden');

    setTimeout(() => {
        el.classList.add('hidden');
    }, 5000);
}

/**
 * Utility functions
 */
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function capitalize(str) {
    return str.charAt(0).toUpperCase() + str.slice(1);
}

function truncate(str, maxLength) {
    if (str.length <= maxLength) return str;
    return str.substring(0, maxLength) + '...';
}

// Initialize on page load
document.addEventListener('DOMContentLoaded', init);
