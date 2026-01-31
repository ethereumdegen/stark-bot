import { LucideIcon, HelpCircle, Activity, Plus, RefreshCw, Trash2, Wand2, Wrench, Cpu, Download, Bug, Check, X, Square } from 'lucide-react';

// Command enum - single source of truth for all command names
export enum Command {
  Help = 'help',
  Status = 'status',
  New = 'new',
  Reset = 'reset',
  Clear = 'clear',
  Stop = 'stop',
  Skills = 'skills',
  Tools = 'tools',
  Model = 'model',
  Export = 'export',
  Debug = 'debug',
  Confirm = 'confirm',
  Cancel = 'cancel',
}

// Command metadata - descriptions and icons
export interface CommandDefinition {
  command: Command;
  name: string;
  description: string;
  icon: LucideIcon;
  category: 'general' | 'session' | 'info' | 'transaction';
}

export const COMMAND_DEFINITIONS: Record<Command, CommandDefinition> = {
  [Command.Help]: {
    command: Command.Help,
    name: 'help',
    description: 'List all available commands',
    icon: HelpCircle,
    category: 'general',
  },
  [Command.Status]: {
    command: Command.Status,
    name: 'status',
    description: 'Show session statistics',
    icon: Activity,
    category: 'info',
  },
  [Command.New]: {
    command: Command.New,
    name: 'new',
    description: 'Start a new conversation',
    icon: Plus,
    category: 'session',
  },
  [Command.Reset]: {
    command: Command.Reset,
    name: 'reset',
    description: 'Reset conversation history',
    icon: RefreshCw,
    category: 'session',
  },
  [Command.Clear]: {
    command: Command.Clear,
    name: 'clear',
    description: 'Clear the chat display',
    icon: Trash2,
    category: 'session',
  },
  [Command.Stop]: {
    command: Command.Stop,
    name: 'stop',
    description: 'Stop the current agent execution',
    icon: Square,
    category: 'session',
  },
  [Command.Skills]: {
    command: Command.Skills,
    name: 'skills',
    description: 'List available skills',
    icon: Wand2,
    category: 'info',
  },
  [Command.Tools]: {
    command: Command.Tools,
    name: 'tools',
    description: 'List available tools',
    icon: Wrench,
    category: 'info',
  },
  [Command.Model]: {
    command: Command.Model,
    name: 'model',
    description: 'Show model configuration',
    icon: Cpu,
    category: 'info',
  },
  [Command.Export]: {
    command: Command.Export,
    name: 'export',
    description: 'Download conversation as JSON',
    icon: Download,
    category: 'general',
  },
  [Command.Debug]: {
    command: Command.Debug,
    name: 'debug',
    description: 'Toggle debug mode',
    icon: Bug,
    category: 'general',
  },
  [Command.Confirm]: {
    command: Command.Confirm,
    name: 'confirm',
    description: 'Confirm pending transaction',
    icon: Check,
    category: 'transaction',
  },
  [Command.Cancel]: {
    command: Command.Cancel,
    name: 'cancel',
    description: 'Cancel pending transaction',
    icon: X,
    category: 'transaction',
  },
};

// Helper to get all commands as an array
export const getAllCommands = (): CommandDefinition[] => {
  return Object.values(COMMAND_DEFINITIONS);
};

// Helper to get commands by category
export const getCommandsByCategory = (category: CommandDefinition['category']): CommandDefinition[] => {
  return getAllCommands().filter((cmd) => cmd.category === category);
};

// Helper to get command definition by name
export const getCommandByName = (name: string): CommandDefinition | undefined => {
  return getAllCommands().find((cmd) => cmd.name === name);
};

// Category labels for display
export const CATEGORY_LABELS: Record<CommandDefinition['category'], string> = {
  general: 'General',
  session: 'Session',
  info: 'Information',
  transaction: 'Transaction',
};

// Order of categories for display
export const CATEGORY_ORDER: CommandDefinition['category'][] = ['general', 'session', 'info'];
