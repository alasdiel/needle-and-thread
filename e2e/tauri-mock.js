// A stand-in for Tauri, so the frontend can be driven in a plain browser against `trunk serve`.
// It answers the commands the workspace and the network board use with the sample vault's
// Tidewater project, held in memory, and keeps every save_board call in window.__saves so a
// test can check what the board wrote. Anything else fails the way a real failed command does.
(() => {
  const project = { slug: 'tidewater', title: 'Tidewater', kind: 'fiction' };
  const scene = (slug, id, title, words) => ({ slug, id, title, status: 'draft', summary: '', words });
  const outline = {
    project,
    statuses: ['idea', 'draft', 'revised', 'done'],
    chapters: [
      { id: 'ol_1', title: 'Arrival', part: 'Part one: Landfall', summary: '', scenes: [scene('night-market', 'sc_1', 'The night market', 239)] },
      { id: 'ol_2', title: 'The long night', part: null, summary: '', scenes: [scene('long-chapter', 'sc_2', 'The long night', 9984)] },
    ],
    unplaced: [],
  };
  const card = (id, path, kind, title, at, turn) => ({
    id, owner: 'tidewater', world: false, path, kind, title, from: '', at, turn,
  });
  const board = {
    cards: [
      card('nt_mara', 'characters/mara-venn', 'character', 'Mara Venn', [0, 0], -1.2),
      card('nt_teodor', 'characters/old-teodor', 'character', 'Old Teodor', [260, 20], 1.0),
      card('nt_market', 'places/night-market', 'place', 'Night Market', [120, 230], -0.6),
      card('nt_ledger', 'threads/the-missing-ledger', 'thread', 'The missing ledger', [420, 240], 1.4),
    ],
    relationships: [
      { id: 'nt_rel', owner: 'tidewater', world: false, path: 'relationships/owes', label: 'owes', from: 'nt_mara', to: 'nt_teodor', directed: true },
    ],
    links: [{ from: 'nt_market', to: 'nt_mara' }],
    zones: [],
    // One of each kind already saved, so drawing what's in network.toml is checked too.
    marks: [
      { kind: 'note', id: 'mk_seed1', text: 'he knows', at: [0, -90], turn: -3, on: 'nt_teodor' },
      { kind: 'ring', id: 'mk_seed2', at: [420, 240], size: [210, 150], turn: 0 },
    ],
  };
  window.__saves = [];

  const answers = {
    appearance: () => 'dark',
    markdown_panel: () => false,
    close_listening: () => null,
    current_vault: () => ({
      path: '/sample-vault',
      projects: [project],
      typography: { double_quotes: true, single_quotes: true, em_dash: true, ellipsis: true },
    }),
    project_outline: () => outline,
    project_notes: () => ({ notes: [], world: null }),
    open_scene: () => ({ scene: outline.chapters[0].scenes[0], markdown: 'The market opened at dusk.' }),
    backup: () => ({ remote: '', after_snapshot: true, status: { state: 'waiting' } }),
    bin_items: () => [],
    // A one-word dictionary: enough for spellcheck to start without a banner in the way.
    spell_dictionary: () => ({ aff: 'SET UTF-8\n', dic: '1\nthe\n', personal_words: [] }),
    project_board: () => structuredClone(board),
    save_board: (args) => {
      const layout = args.layout;
      window.__saves.push(structuredClone(layout));
      board.marks = layout.marks;
      for (const pinned of layout.nodes) {
        const c = board.cards.find((c) => c.id === pinned.id);
        if (c) c.at = pinned.at;
      }
      return null;
    },
  };

  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        const answer = answers[cmd];
        if (!answer) return Promise.reject(`no stand-in for ${cmd}`);
        try {
          return Promise.resolve(answer(args ?? {}));
        } catch (e) {
          return Promise.reject(String(e));
        }
      },
    },
    event: { listen: () => Promise.resolve(() => {}) },
  };
})();
