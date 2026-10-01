local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
local rpc = require('projman.rpc')
local picker, explorer = require('projman.picker'), require('projman.explorer')
vim.notify = function() end
vim.g.mapleader = ' '
local function await(predicate, message) assert(vim.wait(2000, predicate, 5), message or 'Timed out') end
local function settle(ms) vim.wait(ms or 100, function() return false end, 5) end
local function map(key, mode)
  local mapping = vim.fn.maparg(key, mode or 'n', false, true)
  assert(type(mapping.callback) == 'function', 'Missing mapping ' .. key)
  mapping.callback()
end
local function node(id, parent, depth, label)
  return { id = id, title = label or id:upper(), type_key = 'item', type_name = 'Item', depth = depth, parent = parent or vim.NIL,
    archived = false, more = vim.NIL, via = parent and { edge_id = parent .. id, label = 'Relates to', direction = 'outgoing', type_key = 'relates', position = 0 } or vim.NIL }
end
local tree = { root = 'root', graph_revision = 7, nodes = { node('root', nil, 0), node('a', 'root', 1), node('b', 'root', 1), node('c', 'a', 2), node('d', 'b', 2) },
  references = { { from = 'c', to = 'd', via = { edge_id = 'cd', label = 'Relates to', direction = 'outgoing' } },
    { from = 'd', to = 'root', via = { edge_id = 'dr', label = 'Relates to', direction = 'outgoing' } } },
  truncated = { depth = false, nodes = false, references = false } }
local requests, fail = {}, false
rpc.request = function(method, params, callback)
  requests[#requests + 1] = { method = method, params = vim.deepcopy(params) }
  if method == 'explorer.tree' then
    local data = vim.deepcopy(tree); data.options = params
    if params.root ~= 'root' then data.root = params.root; data.nodes = { node(params.root, nil, 0) }; data.references = {} end
    local failure = fail
    vim.defer_fn(function() callback(failure and { message = 'Test connection failure' } or nil, data) end, params.root == 'old' and 100 or 5)
  elseif method == 'node.pick' then
    local items, total, next_offset, delay = {}, 5, vim.NIL, 5
    if params.query == 'slow' then items = { node('stale') }; total = 1; delay = 100
    elseif params.query == 'fast' then items = { node('correct') }; total = 1
    elseif params.query ~= '' then total = 0
    else
      for index = params.offset + 1, math.min(5, params.offset + params.limit) do items[#items + 1] = node('item' .. index) end
      if params.offset + params.limit < total then next_offset = params.offset + params.limit end
    end
    vim.defer_fn(function() callback(nil, { items = items, total = total, next_offset = next_offset, graph_revision = 7 }) end, delay)
  else error('Unexpected method ' .. method) end
end
local chosen
local p = picker.open({ limit = 2, debounce = 1, on_select = function(item) chosen = item.id end })
await(function() return not p.loading end)
assert(#p.items == 2 and p.total == 5)
map('<C-f>', 'i'); await(function() return not p.loading end)
assert(p.offset == 2 and p.items[1].id == 'item3')
map('<C-b>', 'i'); await(function() return not p.loading end); assert(p.offset == 0)
local function query(value)
  vim.api.nvim_buf_set_lines(p.input_buf, 0, -1, false, { value })
  vim.api.nvim_exec_autocmds('TextChangedI', { buffer = p.input_buf })
end
query('slow'); await(function() return requests[#requests].params.query == 'slow' end)
query('fast'); await(function() return not p.loading end)
settle(120)
assert(p.items[1].id == 'correct', 'Late response cannot replace current query results')
map('<CR>', 'i'); assert(chosen == 'correct' and p.closed)
assert(not vim.api.nvim_buf_is_valid(p.input_buf) and not vim.api.nvim_win_is_valid(p.results_win))
p = picker.open({ on_select = function() error('Cancelled picker selected a node') end })
map('<Esc>', 'i'); settle(); assert(p.closed)
p = picker.open({ query = 'nothing matches', on_select = function() error('Empty picker selected a node') end })
await(function() return not p.loading end); map('<CR>', 'i'); assert(not p.closed); picker.close(p)
-- Sidebar must not destroy an ordinary unsaved editing buffer.
vim.cmd('stopinsert')
vim.o.hidden = false
local original_win, original_buf = vim.api.nvim_get_current_win(), vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_lines(original_buf, 0, -1, false, { 'Unsaved ordinary work' })
local opened
explorer.setup({ width = 30 }, function(id, win) opened = { id = id, win = win } end)
local s = explorer.open('root')
await(function() return not s.loading end)
assert(vim.fn.maparg('<Space>', 'n') == '', 'Space leader must not get a sidebar action')
local leader_hit = false
vim.keymap.set('n', '<leader>Pz', function() leader_hit = true end)
vim.api.nvim_feedkeys(' Pz', 'xt', false)
assert(leader_hit, 'Sidebar Space mapping must not swallow LazyVim leader shortcuts')
local function select_node(id, reference)
  for line, row in pairs(s.rows) do if row.id == id and not not row.reference == not not reference then vim.api.nvim_set_current_win(s.win); vim.api.nvim_win_set_cursor(s.win, { line, 0 }); return end end
  error('Row not visible: ' .. id)
end
select_node('a'); map('l'); select_node('c'); map('l'); select_node('d', true)
map('gp'); assert(s.rows[vim.api.nvim_win_get_cursor(s.win)[1]].id == 'd' and not s.rows[vim.api.nvim_win_get_cursor(s.win)[1]].reference)
select_node('a'); map('h'); assert(s.expanded.a == false)
select_node('b'); map('<CR>')
assert(opened.id == 'b' and opened.win ~= s.win and opened.win ~= original_win)
assert(vim.api.nvim_buf_get_lines(original_buf, 0, -1, false)[1] == 'Unsaved ordinary work')
assert(vim.api.nvim_win_get_buf(s.win) == s.buf, 'Opening a node preserves the explorer sidebar')
vim.api.nvim_set_current_win(s.win); map('d'); await(function() return not s.loading end); assert(s.options.direction == 'incoming')
fail = true; map('r'); await(function() return not s.loading end); assert(s.error and s.data, 'Failed refresh retains the last snapshot')
fail = false; map('r'); await(function() return not s.loading end); assert(not s.error)
local before_refresh = #requests
vim.api.nvim_exec_autocmds('User', { pattern = 'ProjManGraphChanged', data = { graph_revision = 8 } })
await(function() return not s.loading end); assert(#requests > before_refresh, 'Successful edits refresh the explorer')
explorer.open('old'); explorer.open('new'); await(function() return not s.loading end); settle(120)
assert(s.data.root == 'new', 'Late root response cannot overwrite a newer root')
local buf = s.buf; explorer.close(s); settle()
assert(not vim.api.nvim_buf_is_valid(buf) and not explorer.state())
-- Closing before a pending response must not resurrect the sidebar.
s = explorer.open('old'); explorer.close(s); settle(120); assert(not explorer.state())
print('Explorer UI checks passed: live filtering, paging, stale responses, folds, references, window safety, refresh errors and cleanup')
vim.cmd('qa!')
