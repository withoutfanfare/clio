// Synthetic data only. Loaded solely by fixture-server.mjs; all IPC is intercepted.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
const now = () => new Date().toISOString();
const memories = Array.from({ length: 125 }, (_, i) => ({ id: `018f675c-379a-70cd-813d-${String(i + 1).padStart(12, '0')}`, namespace: i < 120 ? 'project:demo' : 'project:other', kind: ['fact', 'task', 'constraint', 'note', 'custom-kind'][i % 5], title: `Synthetic memory ${String(i + 1).padStart(3, '0')}`, summary: 'Disposable browser evidence.', content: `Synthetic evidence ${i + 1}. Safe to edit.`, tags: ['demo'], importance: 3, confidence: null, metadata: {}, source: 'fixture', source_ref: null, valid_from: null, valid_until: null, archived_at: i >= 115 && i < 120 ? now() : null, created_at: '2025-08-12T12:00:00Z', updated_at: '2026-09-01T12:00:00Z', rank: null, linked_from: null }));
let inbox = [{ id: '018f675c-379a-70cd-813d-000000000900', content: 'Review this synthetic capture.', suggested_namespace: 'project:demo', suggested_kind: 'fact', suggested_title: 'Synthetic capture awaiting approval', suggested_tags: ['demo'], suggested_importance: 3, suggested_confidence: 0.5, suggested_summary: null, source_route: 'fixture', source_ref: null, metadata: {}, status: 'pending', created_at: now(), reviewed_at: null }];
let action = { id: '018f675c-379a-70cd-813d-000000000800', memory_id: memories[0].id, namespace: 'project:demo', status: 'open', owner: null, trigger: null, waiting_on: null, external_system: null, external_ref: null, reason: 'overdue', completion_condition: 'Confirm synthetic behaviour', created_at: now(), updated_at: now(), due_at: '2026-09-01T09:00:00Z', remind_at: null, resolved_at: null };
function find(id) { const m = memories.find(m => m.id === id); if (!m)
    throw Error('Not found'); return m; }
function recall(a) { let selected = memories.filter(m => (!a.namespace || m.namespace === a.namespace) && (!a.kind || m.kind === a.kind) && (a.archivedOnly ? !!m.archived_at : a.includeArchived || !m.archived_at)); if (a.query)
    selected = selected.filter(m => `${m.title} ${m.content}`.toLowerCase().includes(a.query.toLowerCase())); if (a.tags?.length)
    selected = selected.filter(m => a.tags.every(t => m.tags.includes(t))); selected = selected.filter(m => (!a.importanceMin || m.importance >= a.importanceMin) && (!a.importanceMax || m.importance <= a.importanceMax)); const [field, direction] = (a.sortBy || 'updated_desc').split('_'); const key = field === 'importance' ? 'importance' : `${field}_at`; selected.sort((x, y) => (String(x[key]).localeCompare(String(y[key])) * (direction === 'asc' ? 1 : -1)) || x.id.localeCompare(y.id)); const offset = a.offset || 0, limit = Math.min(a.limit || 10, 100); return { archived_only: !!a.archivedOnly, total: selected.length, count: Math.min(limit, Math.max(0, selected.length - offset)), offset, limit, items: selected.slice(offset, offset + limit) }; }
mockWindows('main');
mockIPC(async (command, a = {}) => {
    if (command.startsWith('plugin:'))
        return null;
    switch (command) {
        case 'cmd_connection_status': return { backend: 'local', label: 'Disposable fixture', connected: true, detail: 'Synthetic data only' };
        case 'cmd_namespaces': return ['project:demo', 'project:other'];
        case 'cmd_namespace_details': return ['project:demo', 'project:other'].map(name => ({ name, memory_count: memories.filter(m => m.namespace === name).length, last_activity: now() }));
        case 'cmd_recent':
        case 'cmd_recall':
        case 'cmd_search': return structuredClone(recall(a));
        case 'cmd_get': return structuredClone(find(a.memoryId));
        case 'cmd_update': {
            const m = find(a.memoryId);
            if (a.patch.content?.includes('[fail]'))
                throw Error('Synthetic save failure: remove [fail] to retry');
            if (a.patch.expected_updated_at !== m.updated_at)
                throw Error('Conflict: record changed elsewhere');
            const { expected_updated_at, ...patch } = a.patch;
            Object.assign(m, patch, { updated_at: now() });
            return structuredClone(m);
        }
        case 'cmd_remember': {
            const m = { ...memories[0], ...a, id: crypto.randomUUID(), namespace: a.namespace || 'global', archived_at: null, updated_at: now(), created_at: now() };
            memories.push(m);
            return structuredClone(m);
        }
        case 'cmd_archive':
        case 'cmd_unarchive': {
            const m = find(a.memoryId);
            m.archived_at = command === 'cmd_archive' ? now() : null;
            return structuredClone(m);
        }
        case 'cmd_get_links':
        case 'cmd_link_contexts':
        case 'cmd_suggest_links':
        case 'cmd_activity': return [];
        case 'cmd_stats': {
            const list = memories.filter(m => !a.namespace || m.namespace === a.namespace);
            const tally = key => [...new Set(list.map(m => m[key]))].map(k => [k, list.filter(m => m[key] === k).length]);
            return { namespace: a.namespace || null, total_memories: list.length, active_memories: list.filter(m => !m.archived_at).length, archived_memories: list.filter(m => m.archived_at).length, total_embeddings: 0, embedding_coverage: 0, by_namespace: tally('namespace'), by_kind: tally('kind'), by_week: [], top_tags: [['demo', list.length]], total_links: 0, link_density: 0 };
        }
        case 'cmd_attention_overview': return { eligible: action ? [action] : [], open: action ? [action] : [], memory_titles: { [memories[0].id]: memories[0].title }, review_pending: inbox.length, consolidation_stale: true, generated_at: now() };
        case 'cmd_action_complete':
        case 'cmd_action_cancel': {
            const old = action;
            action = null;
            return old;
        }
        case 'cmd_action_snooze':
            action = { ...action, remind_at: a.until };
            return action;
        case 'cmd_capture_queue_health': return { pending: 4, processing: null, dead: 2, oldest_pending_age_secs: 7500, scope: 'local', checked_at: now(), unavailable_buckets: ['processing'] };
        case 'cmd_inbox_list': return structuredClone(inbox);
        case 'cmd_inbox_approve': {
            const i = inbox.find(i => i.id === a.reviewId);
            if (!i)
                throw Error('Already reviewed');
            inbox = inbox.filter(i => i.id !== a.reviewId);
            const m = { ...memories[0], id: crypto.randomUUID(), title: i.suggested_title, content: i.content, namespace: i.suggested_namespace };
            memories.push(m);
            return m;
        }
        case 'cmd_inbox_reject': {
            const i = inbox.find(i => i.id === a.reviewId);
            inbox = inbox.filter(i => i.id !== a.reviewId);
            return i;
        }
        case 'cmd_inbox_edit': {
            const i = inbox.find(i => i.id === a.reviewId);
            if (!i)
                throw Error('Already reviewed');
            for (const k of ['namespace', 'kind', 'title', 'summary', 'tags', 'importance'])
                if (a[k] !== undefined)
                    i[`suggested_${k}`] = a[k];
            i.status = 'edited';
            return structuredClone(i);
        }
        case 'cmd_capture': {
            const item = { ...inbox[0], id: crypto.randomUUID(), content: a.text, suggested_namespace: a.namespace || 'global', suggested_kind: 'note', suggested_title: 'Synthetic captured text', suggested_tags: [], suggested_importance: 3, status: 'pending', created_at: now() };
            inbox.push(item);
            return { ...item, outcome: 'Queued' };
        }
        case 'cmd_copy_to_clipboard': return navigator.clipboard.writeText(a.text);
        case 'cmd_capture_preferences': return { enabled: true, model: 'fixture', review_threshold: 0.8 };
        default: throw Error(`Fixture does not implement ${command}`);
    }
}, { shouldMockEvents: true });
void import('/src/main.ts');
