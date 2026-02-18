// Barrel re-export — all existing `import { ... } from '@/lib/api'` continue to work.
export { API_BASE, apiFetch } from './core';
export type { ConfigStatus } from './core';
export { getConfigStatus } from './core';

export * from './auth';
export * from './chat';
export * from './settings';
export * from './tools';
export * from './skills';
export * from './sessions';
export * from './memories';
export * from './identities';
export * from './channels';
export * from './keys';
export * from './scheduling';
export * from './tasks';
export * from './files';
export * from './transactions';
export * from './mindmap';
export * from './agent-subtypes';
export * from './kanban';
export * from './system';
export * from './special-roles';
