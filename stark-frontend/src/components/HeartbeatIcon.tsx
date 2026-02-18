import { Heart } from 'lucide-react';
import { useHeartbeatPulse } from '@/hooks/useHeartbeatPulse';

interface HeartbeatIconProps {
  enabled: boolean;
  size?: number;
  className?: string;
}

/**
 * Heart icon that animates when heartbeat events occur.
 * Shared between sidebar and impulse_map header.
 */
export default function HeartbeatIcon({ enabled, size = 16, className = '' }: HeartbeatIconProps) {
  const { isPulsing } = useHeartbeatPulse();

  const colorClass = enabled ? 'text-red-500 fill-red-500' : 'text-slate-500';
  const animateClass = isPulsing ? 'animate-heartbeat' : 'group-hover:animate-heartbeat';

  return (
    <Heart
      size={size}
      className={`${colorClass} ${animateClass} ${className}`}
    />
  );
}
