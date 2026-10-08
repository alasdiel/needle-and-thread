// A stand-in for Tauri, so the frontend can be driven in a plain browser against `trunk serve`.
// It answers the commands the workspace and the network board use with the sample vault's
// Tidewater project, held in memory, and keeps every save_board call in window.__saves so a
// test can check what the board wrote. Commands that change notes (tying a string, a new plot
// point, pinning up) change the board held here, as the real ones change the files, and every
// call is kept in window.__calls. Anything else fails the way a real failed command does.
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
    id, owner: 'tidewater', world: false, path, kind, title, from: '', at, turn, pinned: false,
  });
  const noteView = (n) => ({ owner: n.owner, world: n.world, path: n.path, id: n.id, kind: n.kind, title: n.title, aliases: [], summary: '' });
  // Notes that aren't on the board until pinned: one from the world, one plain note of this
  // project's, and a relationship, which the pin-up list must never offer.
  const offBoard = [
    { id: 'nt_harbour', owner: 'glass-coast', world: true, path: 'places/the-drowned-harbour', kind: 'place', title: 'The Drowned Harbour' },
    { id: 'nt_check', owner: 'tidewater', world: false, path: 'other/things-to-check', kind: 'note', title: 'Things to check' },
  ];
  const board = {
    cards: [
      card('nt_mara', 'characters/mara-venn', 'character', 'Mara Venn', [0, 0], -1.2),
      card('nt_teodor', 'characters/old-teodor', 'character', 'Old Teodor', [260, 20], 1.0),
      card('nt_market', 'places/night-market', 'place', 'Night Market', [120, 230], -0.6),
      card('nt_ledger', 'threads/the-missing-ledger', 'thread', 'The missing ledger', [420, 240], 1.4),
    ],
    relationships: [
      // It changes along the way, so the board shows its last label and the panel its history.
      {
        id: 'nt_rel', owner: 'tidewater', world: false, path: 'relationships/owes', label: 'owes', from: 'nt_mara', to: 'nt_teodor', directed: true,
        begins: null, ends: null, changes: [{ at: 'The night market', label: 'resents' }],
      },
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
  window.__calls = [];
  let made = 0;

  const answers = {
    // Dark unless a test sets window.__appearance before the page loads.
    appearance: () => window.__appearance ?? 'dark',
    markdown_panel: () => false,
    close_listening: () => null,
    current_vault: () => ({
      path: '/sample-vault',
      projects: [project],
      typography: { double_quotes: true, single_quotes: true, em_dash: true, ellipsis: true },
    }),
    project_outline: () => outline,
    project_notes: () => ({
      notes: [
        ...board.cards.map(noteView),
        ...offBoard.map(noteView),
        ...board.relationships.map((r) => noteView({ ...r, kind: 'relationship', title: r.label })),
      ],
      world: ['glass-coast', 'The Glass Coast'],
    }),
    open_note: (a) => {
      const found = [...board.cards, ...offBoard].find((n) => n.path === a.path) ?? board.relationships.find((r) => r.path === a.path);
      if (!found) throw `no note at ${a.path}`;
      return { note: noteView({ kind: 'relationship', title: '', ...found }), markdown: '' };
    },
    note_links: () => ({ appears_in: [], linked_from: [], mentioned_in: [] }),
    add_card: (a) => {
      const id = `nt_new${++made}`;
      const path = `events/${a.title.toLowerCase().replaceAll(' ', '-')}`;
      board.cards.push(card(id, path, a.kind, a.title, a.at, 0));
      return noteView(board.cards.at(-1));
    },
    pin_card: (a) => {
      const note = offBoard.find((n) => n.id === a.id);
      if (!note) throw `nothing to pin with id ${a.id}`;
      board.cards.push({ ...note, from: note.owner === 'tidewater' && !note.world ? '' : note.owner, at: a.at, turn: 0, pinned: true });
      return null;
    },
    unpin_card: (a) => {
      board.cards = board.cards.filter((c) => c.id !== a.id);
      board.marks = board.marks.filter((m) => m.on !== a.id);
      return null;
    },
    add_relationship: (a) => {
      if (a.from === a.to) throw 'a string needs two different cards';
      const id = `nt_tie${++made}`;
      const rel = { id, owner: 'tidewater', world: false, path: `relationships/${id}`, label: a.label.trim(), from: a.from, to: a.to, directed: a.directed, begins: null, ends: null, changes: [] };
      board.relationships.push(rel);
      return noteView({ ...rel, kind: 'relationship', title: id });
    },
    edit_relationship: (a) => {
      const rel = board.relationships.find((r) => r.path === a.path);
      if (!rel) throw `no relationship at ${a.path}`;
      if (a.label != null) rel.label = a.label.trim();
      if (a.directed != null) rel.directed = a.directed;
      if (a.reverse) [rel.from, rel.to] = [rel.to, rel.from];
      return noteView({ ...rel, kind: 'relationship', title: rel.id });
    },
    cut_note: (a) => {
      board.relationships = board.relationships.filter((r) => r.path !== a.path);
      return null;
    },
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
      board.zones = layout.zones;
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
        window.__calls.push({ cmd, args: structuredClone(args ?? {}) });
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
