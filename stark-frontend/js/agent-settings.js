// Agent Settings Page JavaScript

const API_BASE = '';
let providers = [];
let currentSettings = null;

// Check authentication on page load
document.addEventListener('DOMContentLoaded', async () => {
    const token = localStorage.getItem('stark_token');
    if (!token) {
        window.location.href = '/';
        return;
    }

    await loadProviders();
    await loadCurrentSettings();
    setupEventListeners();
});

// Load available providers
async function loadProviders() {
    try {
        const response = await fetch(`${API_BASE}/api/agent-settings/providers`, {
            headers: {
                'Authorization': `Bearer ${localStorage.getItem('stark_token')}`
            }
        });

        if (!response.ok) throw new Error('Failed to load providers');

        providers = await response.json();
        populateProviderSelect();
    } catch (error) {
        console.error('Error loading providers:', error);
        showError('Failed to load provider options');
    }
}

// Populate the provider dropdown
function populateProviderSelect() {
    const select = document.getElementById('provider-select');
    select.innerHTML = '<option value="">Select a provider...</option>';

    providers.forEach(provider => {
        const option = document.createElement('option');
        option.value = provider.id;
        option.textContent = provider.name;
        option.dataset.endpoint = provider.default_endpoint;
        option.dataset.model = provider.default_model;
        select.appendChild(option);
    });
}

// Load current agent settings
async function loadCurrentSettings() {
    try {
        const response = await fetch(`${API_BASE}/api/agent-settings`, {
            headers: {
                'Authorization': `Bearer ${localStorage.getItem('stark_token')}`
            }
        });

        if (!response.ok) throw new Error('Failed to load settings');

        const data = await response.json();

        if (data.configured === false) {
            // No settings configured
            currentSettings = null;
            updateStatusDisplay(null);
        } else {
            currentSettings = data;
            updateStatusDisplay(data);
            populateForm(data);
        }
    } catch (error) {
        console.error('Error loading settings:', error);
        showError('Failed to load current settings');
    }
}

// Update the status card display
function updateStatusDisplay(settings) {
    const providerText = document.getElementById('current-provider');
    const statusBadge = document.getElementById('status-badge');

    if (!settings) {
        providerText.textContent = 'No AI provider configured';
        statusBadge.textContent = 'Not Configured';
        statusBadge.className = 'px-3 py-1 rounded-full text-sm font-medium bg-slate-600 text-slate-300';
    } else {
        const providerName = getProviderName(settings.provider);
        providerText.textContent = `${providerName} - ${settings.model}`;

        if (settings.enabled) {
            statusBadge.textContent = 'Active';
            statusBadge.className = 'px-3 py-1 rounded-full text-sm font-medium bg-green-500/20 text-green-400';
        } else {
            statusBadge.textContent = 'Disabled';
            statusBadge.className = 'px-3 py-1 rounded-full text-sm font-medium bg-yellow-500/20 text-yellow-400';
        }
    }
}

// Get human-readable provider name
function getProviderName(providerId) {
    const provider = providers.find(p => p.id === providerId);
    return provider ? provider.name : providerId;
}

// Populate form with current settings
function populateForm(settings) {
    document.getElementById('provider-select').value = settings.provider;
    document.getElementById('endpoint-input').value = settings.endpoint;
    document.getElementById('model-input').value = settings.model;
    // Don't populate API key for security
}

// Setup event listeners
function setupEventListeners() {
    // Provider selection change
    document.getElementById('provider-select').addEventListener('change', (e) => {
        const selectedOption = e.target.options[e.target.selectedIndex];
        if (selectedOption.value) {
            // Set default endpoint and model
            document.getElementById('endpoint-input').placeholder = selectedOption.dataset.endpoint;
            document.getElementById('model-input').placeholder = selectedOption.dataset.model;

            // Update API key hint
            const hint = document.getElementById('api-key-hint');
            if (selectedOption.value === 'llama') {
                hint.textContent = 'Optional for local Llama/Ollama installation.';
            } else {
                hint.textContent = 'Required. Get your API key from the provider.';
            }
        }
    });

    // Form submission
    document.getElementById('settings-form').addEventListener('submit', async (e) => {
        e.preventDefault();
        await saveSettings();
    });

    // Disable button
    document.getElementById('disable-btn').addEventListener('click', async () => {
        await disableAgent();
    });

    // Logout button
    document.getElementById('logout-btn').addEventListener('click', () => {
        localStorage.removeItem('stark_token');
        window.location.href = '/';
    });
}

// Save settings
async function saveSettings() {
    const provider = document.getElementById('provider-select').value;
    const endpoint = document.getElementById('endpoint-input').value;
    const model = document.getElementById('model-input').value;
    const apiKey = document.getElementById('api-key-input').value;

    if (!provider) {
        showError('Please select a provider');
        return;
    }

    // Get default values if not provided
    const selectedOption = document.getElementById('provider-select').options[
        document.getElementById('provider-select').selectedIndex
    ];

    const requestBody = {
        provider: provider,
        endpoint: endpoint || selectedOption.dataset.endpoint,
        api_key: apiKey,
        model: model || null
    };

    try {
        const response = await fetch(`${API_BASE}/api/agent-settings`, {
            method: 'PUT',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${localStorage.getItem('stark_token')}`
            },
            body: JSON.stringify(requestBody)
        });

        const data = await response.json();

        if (!response.ok) {
            throw new Error(data.error || 'Failed to save settings');
        }

        showSuccess('Settings saved successfully');
        currentSettings = data;
        updateStatusDisplay(data);

        // Clear API key field after successful save
        document.getElementById('api-key-input').value = '';
    } catch (error) {
        console.error('Error saving settings:', error);
        showError(error.message);
    }
}

// Disable agent
async function disableAgent() {
    if (!confirm('Are you sure you want to disable the AI agent? The bot will not respond to messages.')) {
        return;
    }

    try {
        const response = await fetch(`${API_BASE}/api/agent-settings/disable`, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${localStorage.getItem('stark_token')}`
            }
        });

        const data = await response.json();

        if (!response.ok) {
            throw new Error(data.error || 'Failed to disable agent');
        }

        showSuccess('AI agent disabled');
        currentSettings = null;
        updateStatusDisplay(null);
    } catch (error) {
        console.error('Error disabling agent:', error);
        showError(error.message);
    }
}

// Show success message
function showSuccess(message) {
    const successEl = document.getElementById('success-message');
    const errorEl = document.getElementById('error-message');

    errorEl.classList.add('hidden');
    successEl.textContent = message;
    successEl.classList.remove('hidden');

    setTimeout(() => {
        successEl.classList.add('hidden');
    }, 5000);
}

// Show error message
function showError(message) {
    const successEl = document.getElementById('success-message');
    const errorEl = document.getElementById('error-message');

    successEl.classList.add('hidden');
    errorEl.textContent = message;
    errorEl.classList.remove('hidden');

    setTimeout(() => {
        errorEl.classList.add('hidden');
    }, 5000);
}
