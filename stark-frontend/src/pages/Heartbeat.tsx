import React, { useState, useEffect, FormEvent } from 'react';
import { Save, Skull, Heart, AlertCircle, Zap, Users } from 'lucide-react';
import Card, { CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import Button from '@/components/ui/Button';
import {
  getBotSettings,
  updateBotSettings,
  getHeartbeatConfig,
  updateHeartbeatConfig,
  pulseHeartbeatOnce,
  BotSettings as BotSettingsType,
  HeartbeatConfigInfo,
} from '@/lib/api';

export default function Heartbeat() {
  const [, setSettings] = useState<BotSettingsType | null>(null);
  const [rogueModeEnabled, setRogueModeEnabled] = useState(false);
  const [heartbeatConfig, setHeartbeatConfig] = useState<HeartbeatConfigInfo | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);

  useEffect(() => {
    loadSettings();
    loadHeartbeatConfig();
  }, []);

  const loadSettings = async () => {
    try {
      const data = await getBotSettings();
      setSettings(data);
      setRogueModeEnabled(data.rogue_mode_enabled || false);
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to load settings' });
    } finally {
      setIsLoading(false);
    }
  };

  const loadHeartbeatConfig = async () => {
    try {
      const config = await getHeartbeatConfig();
      setHeartbeatConfig(config);
    } catch (err) {
      console.error('Failed to load heartbeat config:', err);
    }
  };

  if (isLoading) {
    return (
      <div className="p-8 flex items-center justify-center">
        <div className="flex items-center gap-3">
          <div className="w-6 h-6 border-2 border-stark-500 border-t-transparent rounded-full animate-spin" />
          <span className="text-slate-400">Loading settings...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="p-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white mb-2">Heartbeat</h1>
        <p className="text-slate-400">Configure operating mode and heartbeat settings</p>
      </div>

      <div className="grid gap-6 max-w-2xl">
        {/* Operating Mode Section */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              {rogueModeEnabled ? (
                <Skull className="w-5 h-5 text-red-400" />
              ) : (
                <Users className="w-5 h-5 text-stark-400" />
              )}
              Operating Mode
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-center justify-between p-4 bg-slate-800/50 rounded-lg">
              <div className="flex items-center gap-3">
                <Users className={`w-5 h-5 ${!rogueModeEnabled ? 'text-stark-400' : 'text-slate-500'}`} />
                <span className={`font-medium ${!rogueModeEnabled ? 'text-white' : 'text-slate-500'}`}>
                  Partner
                </span>
              </div>

              <button
                onClick={async () => {
                  const newValue = !rogueModeEnabled;
                  setIsSaving(true);
                  setMessage(null);
                  try {
                    const updated = await updateBotSettings({
                      rogue_mode_enabled: newValue,
                    });
                    setSettings(updated);
                    setRogueModeEnabled(newValue);
                    setMessage({ type: 'success', text: `Switched to ${newValue ? 'Rogue' : 'Partner'} mode` });
                  } catch (err) {
                    setMessage({ type: 'error', text: 'Failed to update operating mode' });
                  } finally {
                    setIsSaving(false);
                  }
                }}
                disabled={isSaving}
                className={`relative w-14 h-7 rounded-full transition-colors duration-200 ${
                  rogueModeEnabled
                    ? 'bg-red-500'
                    : 'bg-stark-500'
                } ${isSaving ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
              >
                <div
                  className={`absolute top-1 w-5 h-5 rounded-full bg-white transition-transform duration-200 ${
                    rogueModeEnabled ? 'translate-x-8' : 'translate-x-1'
                  }`}
                />
              </button>

              <div className="flex items-center gap-3">
                <span className={`font-medium ${rogueModeEnabled ? 'text-white' : 'text-slate-500'}`}>
                  Rogue
                </span>
                <Skull className={`w-5 h-5 ${rogueModeEnabled ? 'text-red-400' : 'text-slate-500'}`} />
              </div>
            </div>
            <p className="text-xs text-slate-500 mt-2">
              Partner mode: collaborative assistant. Rogue mode: autonomous agent.
            </p>
          </CardContent>
        </Card>

        {/* Heartbeat Section */}
        <HeartbeatSection
          config={heartbeatConfig}
          setConfig={setHeartbeatConfig}
          setMessage={setMessage}
        />

        {message && (
          <div
            className={`px-4 py-3 rounded-lg ${
              message.type === 'success'
                ? 'bg-green-500/20 border border-green-500/50 text-green-400'
                : 'bg-red-500/20 border border-red-500/50 text-red-400'
            }`}
          >
            {message.text}
          </div>
        )}
      </div>
    </div>
  );
}

// Heartbeat Section Component
interface HeartbeatSectionProps {
  config: HeartbeatConfigInfo | null;
  setConfig: React.Dispatch<React.SetStateAction<HeartbeatConfigInfo | null>>;
  setMessage: React.Dispatch<React.SetStateAction<{ type: 'success' | 'error'; text: string } | null>>;
}

function HeartbeatSection({ config, setConfig, setMessage }: HeartbeatSectionProps) {
  const [isSaving, setIsSaving] = useState(false);
  const [isPulsing, setIsPulsing] = useState(false);
  const [countdown, setCountdown] = useState<string | null>(null);

  // Countdown timer effect
  useEffect(() => {
    if (!config?.next_beat_at || !config?.enabled) {
      setCountdown(null);
      return;
    }

    const updateCountdown = () => {
      const now = new Date().getTime();
      const target = new Date(config.next_beat_at!).getTime();
      const diff = target - now;

      if (diff <= 0) {
        setCountdown('soon...');
        return;
      }

      const hours = Math.floor(diff / (1000 * 60 * 60));
      const minutes = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
      const seconds = Math.floor((diff % (1000 * 60)) / 1000);

      if (hours > 0) {
        setCountdown(`${hours}h ${minutes}m ${seconds}s`);
      } else if (minutes > 0) {
        setCountdown(`${minutes}m ${seconds}s`);
      } else {
        setCountdown(`${seconds}s`);
      }
    };

    updateCountdown();
    const interval = setInterval(updateCountdown, 1000);

    return () => clearInterval(interval);
  }, [config?.next_beat_at, config?.enabled]);

  // Helper to convert minutes to value + unit
  const minutesToValueUnit = (minutes: number): { value: number; unit: 'minutes' | 'hours' | 'days' } => {
    if (minutes >= 1440 && minutes % 1440 === 0) {
      return { value: minutes / 1440, unit: 'days' };
    }
    if (minutes >= 60 && minutes % 60 === 0) {
      return { value: minutes / 60, unit: 'hours' };
    }
    return { value: minutes, unit: 'minutes' };
  };

  const initialInterval = minutesToValueUnit(config?.interval_minutes || 60);
  const [intervalValue, setIntervalValue] = useState(initialInterval.value);
  const [intervalUnit, setIntervalUnit] = useState<'minutes' | 'hours' | 'days'>(initialInterval.unit);

  const [formData, setFormData] = useState({
    interval_minutes: config?.interval_minutes || 60,
    active_hours_start: config?.active_hours_start || '09:00',
    active_hours_end: config?.active_hours_end || '17:00',
    active_days: config?.active_days || 'mon,tue,wed,thu,fri',
    enabled: config?.enabled || false,
  });

  useEffect(() => {
    if (config) {
      const interval = minutesToValueUnit(config.interval_minutes);
      setIntervalValue(interval.value);
      setIntervalUnit(interval.unit);
      setFormData({
        interval_minutes: config.interval_minutes,
        active_hours_start: config.active_hours_start || '09:00',
        active_hours_end: config.active_hours_end || '17:00',
        active_days: config.active_days || 'mon,tue,wed,thu,fri',
        enabled: config.enabled,
      });
    }
  }, [config]);

  // Update interval_minutes when value or unit changes
  useEffect(() => {
    const multipliers = { minutes: 1, hours: 60, days: 1440 };
    const minutes = intervalValue * multipliers[intervalUnit];
    setFormData(prev => ({ ...prev, interval_minutes: minutes }));
  }, [intervalValue, intervalUnit]);

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    setIsSaving(true);
    setMessage(null);

    try {
      const updated = await updateHeartbeatConfig(formData);
      setConfig(updated);
      setMessage({ type: 'success', text: 'Heartbeat settings saved' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to update heartbeat config' });
    } finally {
      setIsSaving(false);
    }
  };

  const handlePulseOnce = async () => {
    setIsPulsing(true);
    setMessage(null);
    try {
      const updated = await pulseHeartbeatOnce();
      setConfig(updated);
      setMessage({ type: 'success', text: 'Heartbeat pulse sent' });
    } catch (err) {
      setMessage({ type: 'error', text: 'Failed to pulse heartbeat' });
    } finally {
      setIsPulsing(false);
    }
  };

  const toggleEnabled = async () => {
    if (!config) {
      setMessage({ type: 'error', text: 'Heartbeat config not loaded yet' });
      return;
    }
    setIsSaving(true);
    try {
      const newEnabled = !formData.enabled;
      console.log('[Heartbeat] Toggling enabled:', formData.enabled, '->', newEnabled);
      const updated = await updateHeartbeatConfig({
        enabled: newEnabled,
      });
      console.log('[Heartbeat] Update response:', updated);
      setConfig(updated);
      // Don't manually set formData here - let useEffect handle it from config change
      setMessage({ type: 'success', text: `Heartbeat ${newEnabled ? 'enabled' : 'disabled'}` });
    } catch (err) {
      console.error('[Heartbeat] Toggle failed:', err);
      setMessage({ type: 'error', text: 'Failed to toggle heartbeat' });
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <Card id="heartbeat" className="scroll-mt-20">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2">
            <Heart className="w-5 h-5 text-red-400" />
            Heartbeat
          </CardTitle>
          <div className="flex items-center gap-3">
            {countdown && formData.enabled && (
              <span className="text-sm text-slate-400" title="Time to next pulse">
                {countdown}
              </span>
            )}
            <button
              onClick={toggleEnabled}
              disabled={isSaving || !config}
              className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                formData.enabled ? 'bg-stark-500' : 'bg-slate-600'
              } ${!config ? 'opacity-50 cursor-not-allowed' : ''}`}
            >
              <span
                className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                  formData.enabled ? 'translate-x-6' : 'translate-x-1'
                }`}
              />
            </button>
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <form onSubmit={handleSave} className="space-y-4">
          <div className="bg-slate-800/50 rounded-lg p-3">
            <div className="flex items-start gap-3">
              <AlertCircle className="w-4 h-4 text-stark-400 mt-0.5" />
              <p className="text-xs text-slate-400">
                Periodic check-ins that prompt the agent to review pending tasks and notifications.
              </p>
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-slate-300 mb-2">
              Interval
            </label>
            <div className="flex gap-2">
              <input
                type="number"
                min="1"
                value={intervalValue}
                onChange={(e) => setIntervalValue(parseInt(e.target.value) || 1)}
                className="flex-1 px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
              />
              <select
                value={intervalUnit}
                onChange={(e) => setIntervalUnit(e.target.value as 'minutes' | 'hours' | 'days')}
                className="px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
              >
                <option value="minutes">Minutes</option>
                <option value="hours">Hours</option>
                <option value="days">Days</option>
              </select>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-2">
                Active Hours Start
              </label>
              <input
                type="time"
                value={formData.active_hours_start}
                onChange={(e) => setFormData({ ...formData, active_hours_start: e.target.value })}
                className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-slate-300 mb-2">
                Active Hours End
              </label>
              <input
                type="time"
                value={formData.active_hours_end}
                onChange={(e) => setFormData({ ...formData, active_hours_end: e.target.value })}
                className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded-lg text-white focus:border-stark-500 focus:outline-none"
              />
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-slate-300 mb-2">
              Active Days
            </label>
            <div className="flex flex-wrap gap-2">
              {['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].map((day) => {
                const isActive = formData.active_days.toLowerCase().includes(day);
                return (
                  <button
                    key={day}
                    type="button"
                    onClick={() => {
                      const days = formData.active_days.split(',').map((d) => d.trim().toLowerCase()).filter(d => d);
                      const newDays = isActive
                        ? days.filter((d) => d !== day)
                        : [...days, day];
                      setFormData({ ...formData, active_days: newDays.join(',') });
                    }}
                    className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${
                      isActive
                        ? 'bg-stark-500 text-white'
                        : 'bg-slate-700 text-slate-400 hover:bg-slate-600'
                    }`}
                  >
                    {day.charAt(0).toUpperCase() + day.slice(1)}
                  </button>
                );
              })}
            </div>
          </div>

          {config && (
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <p className="text-slate-500">Last heartbeat</p>
                <p className="text-slate-300">
                  {config.last_beat_at
                    ? new Date(config.last_beat_at).toLocaleString()
                    : 'Never'}
                </p>
              </div>
              <div>
                <p className="text-slate-500">Next heartbeat</p>
                <p className="text-slate-300">
                  {config.next_beat_at
                    ? new Date(config.next_beat_at).toLocaleString()
                    : 'Not scheduled'}
                </p>
              </div>
            </div>
          )}

          <div className="flex gap-2">
            <Button type="submit" isLoading={isSaving} className="w-fit">
              <Save className="w-4 h-4 mr-2" />
              Save
            </Button>
            <Button
              type="button"
              variant="secondary"
              onClick={handlePulseOnce}
              isLoading={isPulsing}
              className="w-fit"
            >
              <Zap className="w-4 h-4 mr-2" />
              Pulse Once
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}
