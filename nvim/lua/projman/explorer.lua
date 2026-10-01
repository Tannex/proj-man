-- A per-tab sidebar over a read-only BFS spanning-tree snapshot.
local M = { instances = {} }
local rpc = require('projman.rpc')
local picker = require('projman.picker')
local defaults = { width = 44, direction = 'outgoing', max_depth = 3, max_nodes = 500, max_references = 1000, include_archived = false }
local settings = vim.deepcopy(defaults)
local open_node
local ns = vim.api.nvim_create_namespace('projman-explorer')
local function valid(win) return win and vim.api.nvim_win_is_valid(win) end
local function clean(text) return tostring(text):gsub('%c', ' ') end
local function state() return M.instances[vim.api.nvim_get_current_tabpage()] end
local function selected(s)
  if not s or not valid(s.win) then return end
  return s.rows[vim.api.nvim_win_get_cursor(s.win)[1]]
end
local function selected_id(s) local row = selected(s); return row and row.id end
local function status(s)
  if s.loading then return 'Loading…' end
  if s.error then return 'Error: ' .. clean(s.error) .. ' · r retry' end
  if not s.data then return 'Choose a root with s' end
  local t = s.data.truncated
  local limits = {}
  if t.depth then limits[#limits + 1] = 'depth' end
  if t.nodes then limits[#limits + 1] = 'nodes' end
  if t.references then limits[#limits + 1] = 'references' end
  return #s.data.nodes .. ' nodes · revision ' .. s.data.graph_revision .. (#limits > 0 and (' · limited: ' .. table.concat(limits, ', ')) or '')
end
local function render(s, focus)
  if s.closed or not valid(s.win) then return end
  focus = focus or selected_id(s)
  local lines = { 'ProjMan · ' .. s.options.direction .. ' · ' .. (s.options.relation or s.options.family or 'all relationships'),
    status(s), 'Enter open · Tab fold · s root · ? help' }
  local highlights = {}
  s.rows = {}
  if s.data then
    local children, references = {}, {}
    s.nodes = {}
    for _, node in ipairs(s.data.nodes) do
      s.nodes[node.id] = node
      if node.parent and node.parent ~= vim.NIL then
        children[node.parent] = children[node.parent] or {}; table.insert(children[node.parent], node)
      end
    end
    for _, ref in ipairs(s.data.references) do
      references[ref.from] = references[ref.from] or {}; table.insert(references[ref.from], ref)
    end
    local function row(node, prefix, branch, reference, via)
      local kids, refs = children[node.id] or {}, references[node.id] or {}
      local more = node.more and node.more ~= vim.NIL
      local expandable = #kids > 0 or #refs > 0 or more
      local marker = reference and '↪' or expandable and (s.expanded[node.id] and '▾' or '▸') or '·'
      local relation = via and ((via.direction == 'incoming' and '← ' or '→ ') .. clean(via.label) .. ': ') or ''
      lines[#lines + 1] = prefix .. branch .. marker .. ' ' .. relation .. clean(node.title) .. ' [' .. clean(node.type_key) .. ']'
        .. (reference and ' (reference)' or more and ' …' or '') .. (node.archived and ' [archived]' or '')
      local line = #lines
      s.rows[line] = { id = node.id, reference = reference, parent = reference and via.from or node.parent, node = node, via = via, expandable = expandable }
      if reference then highlights[#highlights + 1] = line end
      if not reference and s.expanded[node.id] then
        local entries = {}
        for _, child in ipairs(kids) do entries[#entries + 1] = { node = child, via = child.via } end
        for _, ref in ipairs(refs) do
          local link = vim.deepcopy(ref.via); link.from = ref.from
          entries[#entries + 1] = { node = s.nodes[ref.to], via = link, reference = true }
        end
        for index, entry in ipairs(entries) do
          local last = index == #entries
          local child_prefix = prefix .. (branch == '' and '' or branch == '└─ ' and '   ' or '│  ')
          row(entry.node, child_prefix, last and '└─ ' or '├─ ', entry.reference, entry.via)
        end
      end
    end
    if s.nodes[s.data.root] then row(s.nodes[s.data.root], '', '', false, nil) end
  end
  vim.bo[s.buf].modifiable = true
  vim.api.nvim_buf_set_lines(s.buf, 0, -1, false, lines)
  vim.bo[s.buf].modifiable = false
  vim.api.nvim_buf_clear_namespace(s.buf, ns, 0, -1)
  vim.api.nvim_buf_set_extmark(s.buf, ns, 0, 0, { end_row = 1, hl_group = 'Title', hl_eol = true })
  for _, line in ipairs(highlights) do vim.api.nvim_buf_set_extmark(s.buf, ns, line - 1, 0, { end_row = line, hl_group = 'Comment', hl_eol = true }) end
  local target = math.min(4, #lines)
  for line = 4, #lines do if s.rows[line] and s.rows[line].id == focus and not s.rows[line].reference then target = line; break end end
  vim.api.nvim_win_set_cursor(s.win, { target, 0 })
end
local function load(s, reset)
  if s.closed then return end
  s.generation = s.generation + 1
  local generation, focus = s.generation, selected_id(s)
  s.loading, s.error = true, nil
  if reset then s.data = nil; s.nodes = {}; s.expanded = { [s.root] = true }; focus = s.root end
  render(s, focus)
  local params = vim.tbl_extend('force', {}, s.options, { root = s.root })
  params.width = nil
  rpc.request('explorer.tree', params, function(err, data)
    if s.closed or generation ~= s.generation or not valid(s.win) then return end
    s.loading = false
    if err then s.error = err.message else s.data = data end
    render(s, focus)
  end)
end
local function focus_node(s, id)
  local node = s.nodes[id]
  if not node then return end
  while node.parent and node.parent ~= vim.NIL and s.nodes[node.parent] do
    s.expanded[node.parent] = true; node = s.nodes[node.parent]
  end
  render(s, id)
end
local function expand(s, toggle)
  local row = selected(s); if not row then return end
  if row.reference then focus_node(s, row.id); return end
  if toggle and s.expanded[row.id] then s.expanded[row.id] = false; render(s, row.id); return end
  s.expanded[row.id] = true
  local more = row.node.more
  if more and more ~= vim.NIL then
    if more == 'depth' and s.options.max_depth < 32 then s.options.max_depth = s.options.max_depth + 1
    elseif more == 'nodes' and s.options.max_nodes < 5000 then s.options.max_nodes = math.min(5000, s.options.max_nodes * 2)
    else vim.notify('Explorer limit reached; narrow the filter or choose a closer root', vim.log.levels.INFO); render(s, row.id); return end
    load(s, false)
  else render(s, row.id) end
end
local function collapse(s)
  local row = selected(s); if not row then return end
  if not row.reference and s.expanded[row.id] and row.expandable then s.expanded[row.id] = false; render(s, row.id)
  elseif row.parent and row.parent ~= vim.NIL then focus_node(s, row.parent) end
end
local function editor(s)
  if not valid(s.editor_win) or vim.api.nvim_win_get_tabpage(s.editor_win) ~= s.tab then
    vim.api.nvim_win_call(s.win, function() vim.cmd('rightbelow vsplit'); s.editor_win = vim.api.nvim_get_current_win(); vim.api.nvim_win_set_buf(s.editor_win, vim.api.nvim_create_buf(true, false)) end)
  end
  local buf = vim.api.nvim_win_get_buf(s.editor_win)
  if vim.bo[buf].modified and vim.bo[buf].bufhidden ~= 'hide' and not vim.o.hidden then
    vim.api.nvim_win_call(s.editor_win, function() vim.cmd('rightbelow vsplit'); s.editor_win = vim.api.nvim_get_current_win(); vim.api.nvim_win_set_buf(s.editor_win, vim.api.nvim_create_buf(true, false)) end)
  end
  return s.editor_win
end
local function open_selected(s)
  local row = selected(s); if not row then return end
  local win = editor(s)
  vim.api.nvim_set_current_win(win)
  open_node(row.id, win)
end
function M.close(s)
  s = s or state(); if not s or s.closed then return end
  s.closed = true; s.generation = s.generation + 1
  M.instances[s.tab] = nil
  if valid(s.win) then
    if #vim.api.nvim_tabpage_list_wins(s.tab) > 1 then pcall(vim.api.nvim_win_close, s.win, true)
    else vim.api.nvim_win_set_buf(s.win, vim.api.nvim_create_buf(true, false)) end
  end
  if vim.api.nvim_buf_is_valid(s.buf) then pcall(vim.api.nvim_buf_delete, s.buf, { force = true }) end
  if s.group then pcall(vim.api.nvim_del_augroup_by_id, s.group) end
end
function M.setup(opts, callback)
  settings = vim.tbl_extend('force', {}, defaults, opts or {})
  open_node = callback
end
function M.pick_root()
  local s = state()
  return picker.open({ include_archived = s and s.options.include_archived or settings.include_archived,
    on_select = function(item) M.open(item.id) end })
end
function M.refresh() local s = state(); if s then load(s, false) else M.pick_root() end end
local function filter(s)
  rpc.request('type.list', {}, function(err, types)
    if s.closed then return end
    if err then vim.notify(err.message, vim.log.levels.ERROR); return end
    local entries, seen = { { label = 'All relationships' } }, {}
    for _, relation in ipairs(types.relationship_types) do
      local family = relation.family ~= '' and relation.family or relation.key
      if not seen[family] then entries[#entries + 1] = { label = 'Family: ' .. family, family = family }; seen[family] = true end
      entries[#entries + 1] = { label = 'Relationship: ' .. relation.name .. ' (' .. relation.key .. ')', relation = relation.key }
    end
    vim.ui.select(entries, { prompt = 'Explorer relationships', format_item = function(item) return item.label end }, function(item)
      if item and not s.closed then s.options.family, s.options.relation = item.family, item.relation; load(s, true) end
    end)
  end)
end
local function help()
  vim.notify(table.concat({
    'ProjMan explorer', 'Enter: open node in the editing window', 'Tab/za: collapse or expand; l/Right: expand; h/Left: collapse or parent',
    's or /: filter and pick a root; R: make selected node the root', 'r: refresh; d: outgoing → incoming → both; t: relationship filter',
    '+ / -: traversal depth; a: include archived nodes; gp: jump to the primary occurrence',
    'q: close explorer. Extra graph links are reference rows, not additional parents.',
  }, '\n'), vim.log.levels.INFO)
end
function M.open(root)
  if not root or root == '' then return M.pick_root() end
  if picker.active then picker.close(picker.active) end
  local tab = vim.api.nvim_get_current_tabpage()
  local s = M.instances[tab]
  if not s or s.closed or not valid(s.win) then
    s = { tab = tab, editor_win = vim.api.nvim_get_current_win(), options = vim.deepcopy(settings), expanded = {}, rows = {}, nodes = {}, generation = 0 }
    s.buf = vim.api.nvim_create_buf(false, true)
    vim.api.nvim_buf_set_name(s.buf, 'projman-explorer://' .. tab)
    vim.bo[s.buf].buftype = 'nofile'; vim.bo[s.buf].bufhidden = 'wipe'; vim.bo[s.buf].swapfile = false; vim.bo[s.buf].filetype = 'projman-explorer'
    vim.cmd('topleft vsplit')
    s.win = vim.api.nvim_get_current_win()
    vim.api.nvim_win_set_buf(s.win, s.buf)
    vim.api.nvim_win_set_width(s.win, math.min(s.options.width, math.max(20, math.floor(vim.o.columns / 2))))
    vim.wo[s.win].winfixwidth = true; vim.wo[s.win].number = false; vim.wo[s.win].relativenumber = false
    vim.wo[s.win].signcolumn = 'no'; vim.wo[s.win].wrap = false; vim.wo[s.win].cursorline = true
    M.instances[tab] = s
    s.group = vim.api.nvim_create_augroup('ProjManExplorer' .. tab, { clear = true })
    vim.api.nvim_create_autocmd('User', { group = s.group, pattern = 'ProjManGraphChanged', callback = function(event)
      local revision = type(event.data) == 'table' and tonumber(event.data.graph_revision) or nil
      if not s.closed and (not s.data or not revision or revision > s.data.graph_revision) then load(s, false) end
    end })
    vim.api.nvim_create_autocmd('WinClosed', { group = s.group, pattern = tostring(s.win), callback = function() M.close(s) end })
    vim.api.nvim_create_autocmd('BufWipeout', { group = s.group, buffer = s.buf, callback = function() M.close(s) end })
    local function map(key, fn, desc) vim.keymap.set('n', key, fn, { buffer = s.buf, silent = true, nowait = true, desc = desc }) end
    map('<CR>', function() open_selected(s) end, 'Open node')
    local fold_keys = { '<Tab>', 'za' }
    -- Do not intercept LazyVim's Space leader or its which-key trigger.
    if vim.g.mapleader ~= ' ' and vim.g.mapleader ~= '<Space>' then fold_keys[#fold_keys + 1] = '<Space>' end
    for _, key in ipairs(fold_keys) do map(key, function() expand(s, true) end, 'Toggle branch') end
    for _, key in ipairs({ 'l', '<Right>' }) do map(key, function() expand(s, false) end, 'Expand branch') end
    for _, key in ipairs({ 'h', '<Left>' }) do map(key, function() collapse(s) end, 'Collapse branch or parent') end
    for _, key in ipairs({ 's', '/' }) do map(key, M.pick_root, 'Choose BFS root') end
    map('R', function() local id = selected_id(s); if id then M.open(id) end end, 'Root at selected node')
    map('r', M.refresh, 'Refresh graph')
    map('d', function() s.options.direction = ({ outgoing = 'incoming', incoming = 'both', both = 'outgoing' })[s.options.direction]; load(s, true) end, 'Change traversal direction')
    map('t', function() filter(s) end, 'Filter relationships')
    map('a', function() s.options.include_archived = not s.options.include_archived; load(s, true) end, 'Toggle archived nodes')
    map('+', function() s.options.max_depth = math.min(32, s.options.max_depth + 1); load(s, false) end, 'Increase BFS depth')
    map('-', function() s.options.max_depth = math.max(0, s.options.max_depth - 1); load(s, false) end, 'Decrease BFS depth')
    map('gp', function() local id = selected_id(s); if id then focus_node(s, id) end end, 'Jump to primary occurrence')
    map('gg', function() vim.api.nvim_win_set_cursor(s.win, { math.min(4, vim.api.nvim_buf_line_count(s.buf)), 0 }) end, 'First node')
    map('?', help, 'Explorer help')
    map('q', function() M.close(s) end, 'Close explorer')
  else vim.api.nvim_set_current_win(s.win) end
  s.root = root
  load(s, true)
  return s
end
function M.toggle()
  if picker.active then picker.close(picker.active); return end
  local s = state()
  if s then M.close(s) else M.pick_root() end
end
function M.state() return state() end
function M.editing_window()
  local s = state()
  if s and vim.api.nvim_get_current_win() == s.win then return editor(s) end
  return vim.api.nvim_get_current_win()
end
return M
