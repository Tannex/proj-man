-- Native, asynchronous root picker. No external fuzzy-picker plugin is required.
local M = {}
local rpc = require('projman.rpc')
local sequence = 0
local function clean(value) return tostring(value):gsub('%c', ' ') end
local function valid(win) return win and vim.api.nvim_win_is_valid(win) end
local function input(s)
  if not vim.api.nvim_buf_is_valid(s.input_buf) then return '' end
  return table.concat(vim.api.nvim_buf_get_lines(s.input_buf, 0, -1, false), ' ')
end
local function render(s)
  if s.closed or not valid(s.results_win) then return end
  local lines = {}
  for _, item in ipairs(s.items) do
    lines[#lines + 1] = clean(item.title) .. '  [' .. clean(item.type_name or item.type_key) .. ']  ' .. item.id:sub(1, 8)
  end
  if #lines == 0 then lines = { s.loading and 'Loading…' or s.error and ('Error: ' .. clean(s.error)) or 'No matching nodes' } end
  vim.bo[s.results_buf].modifiable = true
  vim.api.nvim_buf_set_lines(s.results_buf, 0, -1, false, lines)
  vim.bo[s.results_buf].modifiable = false
  local title = s.loading and ' Searching… ' or (' ' .. (s.total or 0) .. ' matches · ' .. (s.offset + (#s.items > 0 and 1 or 0)) .. '–' .. (s.offset + #s.items) .. ' ')
  if s.error then title = ' Search failed · Ctrl-R retries ' end
  vim.api.nvim_win_set_config(s.results_win, { title = title })
  s.index = math.max(1, math.min(s.index, #lines))
  vim.api.nvim_win_set_cursor(s.results_win, { s.index, 0 })
end
function M.close(s)
  s = s or M.active
  if not s or s.closed then return end
  s.closed = true
  s.generation = s.generation + 1
  if s.timer then s.timer:stop(); s.timer:close(); s.timer = nil end
  if M.active == s then M.active = nil end
  -- Only leave insert mode if this picker owns the current window.
  local current = vim.api.nvim_get_current_win()
  local owned = current == s.input_win or current == s.results_win
  if owned then vim.cmd('stopinsert') end
  for _, win in ipairs({ s.results_win, s.input_win }) do if valid(win) then pcall(vim.api.nvim_win_close, win, true) end end
  for _, buf in ipairs({ s.results_buf, s.input_buf }) do if vim.api.nvim_buf_is_valid(buf) then pcall(vim.api.nvim_buf_delete, buf, { force = true }) end end
  pcall(vim.api.nvim_del_augroup_by_id, s.group)
  if owned and valid(s.origin_win) then vim.api.nvim_set_current_win(s.origin_win) end
end
local function choose(s)
  if s.closed or s.loading or input(s) ~= s.query then return end
  local index = vim.api.nvim_get_current_win() == s.results_win and vim.api.nvim_win_get_cursor(s.results_win)[1] or s.index
  local item = s.items[index]
  if not item then return end
  M.close(s)
  s.on_select(item)
end
local function move(s, step)
  if s.loading or #s.items == 0 then return end
  s.index = math.max(1, math.min(#s.items, s.index + step))
  vim.api.nvim_win_set_cursor(s.results_win, { s.index, 0 })
end
local function fetch(s, generation)
  if s.closed or s.generation ~= generation then return end
  s.request('node.pick', { query = s.query, offset = s.offset, limit = s.limit, include_archived = s.include_archived }, function(err, data)
    if s.closed or generation ~= s.generation or input(s) ~= s.query then return end
    s.loading = false
    if err then s.error = err.message; s.items = {}; s.next_offset = nil
    else s.items, s.total, s.next_offset = data.items, data.total, data.next_offset ~= vim.NIL and data.next_offset or nil end
    render(s)
  end)
end
local function search(s, offset, debounce)
  if s.closed then return end
  if s.timer then s.timer:stop(); s.timer:close(); s.timer = nil end
  s.generation = s.generation + 1
  s.query, s.offset, s.index = input(s), offset or 0, 1
  s.items, s.error, s.next_offset, s.loading = {}, nil, nil, true
  render(s)
  local generation = s.generation
  if debounce then
    s.timer = vim.uv.new_timer()
    s.timer:start(s.debounce, 0, vim.schedule_wrap(function() fetch(s, generation) end))
  else fetch(s, generation) end
end
local function page(s, step)
  if s.loading or input(s) ~= s.query then return end
  if step > 0 then if s.next_offset then search(s, s.next_offset) end
  elseif s.offset > 0 then search(s, math.max(0, s.offset - s.limit)) end
end
function M.open(opts)
  opts = opts or {}
  if M.active then M.close(M.active) end
  if vim.o.lines < 8 or vim.o.columns < 20 then vim.notify('More screen space is needed for the root picker', vim.log.levels.WARN); return end
  sequence = sequence + 1
  local height = math.max(1, math.min(12, vim.o.lines - 7))
  local width = math.max(1, math.min(90, vim.o.columns - 4))
  local row = math.max(0, math.floor((vim.o.lines - height - 6) / 2))
  local col = math.max(0, math.floor((vim.o.columns - width - 2) / 2))
  local s = { origin_win = vim.api.nvim_get_current_win(), generation = 0, query = '', offset = 0, index = 1, items = {},
    on_select = assert(opts.on_select), request = opts.request or rpc.request, limit = opts.limit or 50,
    include_archived = opts.include_archived or false, debounce = opts.debounce or 120 }
  M.active = s
  s.group = vim.api.nvim_create_augroup('ProjManRootPicker' .. sequence, { clear = true })
  s.input_buf = vim.api.nvim_create_buf(false, true)
  s.results_buf = vim.api.nvim_create_buf(false, true)
  for _, buf in ipairs({ s.input_buf, s.results_buf }) do
    vim.bo[buf].buftype = 'nofile'; vim.bo[buf].bufhidden = 'wipe'; vim.bo[buf].swapfile = false; vim.bo[buf].undolevels = -1
  end
  vim.api.nvim_buf_set_name(s.input_buf, 'projman-picker://query/' .. sequence)
  vim.api.nvim_buf_set_name(s.results_buf, 'projman-picker://results/' .. sequence)
  vim.api.nvim_buf_set_lines(s.input_buf, 0, -1, false, { opts.query or '' })
  s.results_win = vim.api.nvim_open_win(s.results_buf, false, { relative = 'editor', row = row + 3, col = col, width = width,
    height = height, style = 'minimal', border = 'rounded', title = ' Roots ', footer = ' Enter select · Esc cancel · Ctrl-F/B page · Ctrl-R retry ' })
  s.input_win = vim.api.nvim_open_win(s.input_buf, true, { relative = 'editor', row = row, col = col, width = width,
    height = 1, style = 'minimal', border = 'rounded', title = ' BFS root · filter title, type, or ID ' })
  vim.wo[s.results_win].cursorline = true
  vim.wo[s.results_win].wrap = false; vim.wo[s.input_win].wrap = false
  vim.api.nvim_create_autocmd({ 'TextChangedI', 'TextChanged' }, { group = s.group, buffer = s.input_buf, callback = function() search(s, 0, true) end })
  for _, buf in ipairs({ s.input_buf, s.results_buf }) do
    local function map(keys, fn) vim.keymap.set({ 'n', 'i' }, keys, fn, { buffer = buf, silent = true, nowait = true }) end
    map('<CR>', function() choose(s) end)
    map('<Esc>', function() M.close(s) end)
    map('<C-c>', function() M.close(s) end)
    map('<Down>', function() move(s, 1) end); map('<C-n>', function() move(s, 1) end); map('<C-j>', function() move(s, 1) end)
    map('<Up>', function() move(s, -1) end); map('<C-p>', function() move(s, -1) end); map('<C-k>', function() move(s, -1) end)
    map('<C-f>', function() page(s, 1) end); map('<PageDown>', function() page(s, 1) end)
    map('<C-b>', function() page(s, -1) end); map('<PageUp>', function() page(s, -1) end)
    map('<C-r>', function() search(s, s.offset) end)
  end
  vim.keymap.set('n', 'q', function() M.close(s) end, { buffer = s.results_buf })
  vim.keymap.set('n', '/', function() if valid(s.input_win) then vim.api.nvim_set_current_win(s.input_win); vim.cmd('startinsert!') end end, { buffer = s.results_buf })
  vim.api.nvim_create_autocmd('WinClosed', { group = s.group, pattern = { tostring(s.input_win), tostring(s.results_win) }, callback = function() M.close(s) end })
  vim.api.nvim_create_autocmd('VimResized', { group = s.group, callback = function() M.close(s) end })
  search(s, 0)
  vim.api.nvim_win_set_cursor(s.input_win, { 1, #(opts.query or '') })
  vim.cmd('startinsert!')
  return s
end
return M
