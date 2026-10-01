local rpc = require('projman.rpc')
local recovery = require('projman.recovery')
local M = { buffers = {}, history = {} }
local options = { map_tab = true, recovery_delay = 300, notify_save = true, guided_entry = true, entry_mode = 'prompts' }
local ns = vim.api.nvim_create_namespace('projman')
local field_ns = vim.api.nvim_create_namespace('projman-fields')
local save_ns = vim.api.nvim_create_namespace('projman-save')
local initialized = false
local function notify(err)
  if err then vim.notify(type(err) == 'table' and err.message or tostring(err), vim.log.levels.ERROR) end
end
local function uuid()
  local s = vim.fn.sha256(tostring(vim.uv.hrtime()) .. tostring(math.random()) .. tostring(vim.fn.getpid()))
  return s:sub(1, 8) .. '-' .. s:sub(9, 12) .. '-4' .. s:sub(14, 16) .. '-a' .. s:sub(18, 20) .. '-' .. s:sub(21, 32)
end
local function ready()
  if not initialized then M.setup({}) end
end
local function alive(buf) return vim.api.nvim_buf_is_valid(buf) end
local function text(buf) return table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), '\n') end
local function current()
  local buf = vim.api.nvim_get_current_buf()
  local state = M.buffers[buf]
  if not state then notify('Open a ProjMan node first'); return end
  return buf, state
end
local function object() return vim.empty_dict() end
local function fields(buf, state)
  local spans, rows = {}, {}
  for line, content in ipairs(vim.api.nvim_buf_get_lines(buf, 0, -1, false)) do
    if line > 1 and content == '---' then break end
    local key, spaces = content:match('^([a-z][a-z0-9_]*):(%s*)')
    if key then rows[key] = { line = line, column = #key + 2 + #spaces } end
  end
  for _, definition in ipairs(state.definition.properties) do
    if rows[definition.key] then
      local span = rows[definition.key]
      span.key, span.required = definition.key, definition.required
      table.insert(spans, span)
    end
  end
  return spans
end
local function annotations(buf, state)
  vim.api.nvim_buf_clear_namespace(buf, field_ns, 0, -1)
  for _, field in ipairs(fields(buf, state)) do
    if field.required then
      vim.api.nvim_buf_set_extmark(buf, field_ns, field.line - 1, 0, { virt_text = { { ' required', 'Comment' } }, virt_text_pos = 'eol' })
    end
  end
end
local function save_status(buf, state, phase, message)
  state.save_status = { phase = phase, message = message, revision = state.node.revision }
  if not alive(buf) then return end
  vim.b[buf].projman_save_status = state.save_status
  vim.api.nvim_buf_clear_namespace(buf, save_ns, 0, -1)
  local highlight = phase == 'error' and 'DiagnosticError' or phase == 'unconfirmed' and 'DiagnosticWarn' or phase == 'saved' and 'DiagnosticOk' or 'DiagnosticInfo'
  vim.api.nvim_buf_set_extmark(buf, save_ns, 0, 0, { virt_text = { { ' ' .. message:gsub('%c', ' '), highlight } }, virt_text_pos = 'eol' })
end
local function save_error(buf, state, err, tick)
  state.saving, state.error = false, err
  local unconfirmed = state.pending ~= nil
  local message = unconfirmed and ('Save outcome unconfirmed: ' .. err.message .. ' Use :ProjManRetry.') or ('Not saved: ' .. err.message)
  save_status(buf, state, unconfirmed and 'unconfirmed' or 'error', message)
  if alive(buf) and (not tick or vim.api.nvim_buf_get_changedtick(buf) == tick) and not unconfirmed then
    local details = type(err.details) == 'table' and err.details or {}
    local row = math.max(0, math.min((tonumber(details.line) or 1) - 1, vim.api.nvim_buf_line_count(buf) - 1))
    local line = vim.api.nvim_buf_get_lines(buf, row, row + 1, false)[1] or ''
    local col = math.max(0, math.min((tonumber(details.column) or 1) - 1, #line))
    vim.diagnostic.set(ns, buf, { { lnum = row, col = col, message = err.message, severity = vim.diagnostic.severity.ERROR } })
  end
  vim.notify(message, unconfirmed and vim.log.levels.WARN or vim.log.levels.ERROR, { title = 'ProjMan' })
end
local function validate(buf, state)
  local tick = vim.api.nvim_buf_get_changedtick(buf)
  rpc.request('document.parse', { type_key = state.node.type_key, schema_revision = state.node.schema_revision, text = text(buf) }, function(err, result)
    if not alive(buf) or vim.api.nvim_buf_get_changedtick(buf) ~= tick then return end
    local diagnostics = {}
    if err then
      local details = type(err.details) == 'table' and err.details or {}
      table.insert(diagnostics, { lnum = math.max(0, (details.line or 1) - 1), col = math.max(0, (details.column or 1) - 1), message = err.message, severity = vim.diagnostic.severity.ERROR })
    else
      if result.active_schema_revision ~= state.node.schema_revision then
        table.insert(diagnostics, { lnum = 0, col = 0, message = 'Schema changed; migrate this node explicitly before saving', severity = vim.diagnostic.severity.WARN })
      end
      for _, key in ipairs(result.missing) do
        for _, span in ipairs(fields(buf, state)) do
          if key == span.key then table.insert(diagnostics, { lnum = span.line - 1, col = span.column - 1, message = key .. ' is required for completeness', severity = vim.diagnostic.severity.WARN }) end
        end
      end
    end
    vim.diagnostic.set(ns, buf, diagnostics)
    annotations(buf, state)
  end)
end
local function recovery_record(buf, state, pending)
  return {
    id = pending and pending.recovery_id or state.recovery_id,
    workspace = rpc.workspace,
    node_id = state.node.id, type_key = state.node.type_key,
    expected_schema = state.active_schema, expected_revision = state.node.revision,
    expected_nodes = vim.deepcopy(state.expected_nodes),
    text = pending and pending.text or text(buf),
    staged_operations = vim.deepcopy(state.staged),
    pending_mutation = pending and pending.request or nil,
  }
end
local function recover_local(buf, state)
  if rpc.workspace then local _, err = recovery.save(recovery_record(buf, state)); if err then notify(err) end end
end
local function changed(buf, state)
  if not state.saving and not state.pending and state.save_status and state.save_status.phase == 'saved' and vim.bo[buf].modified then
    save_status(buf, state, 'modified', 'Unsaved changes')
  end
  if state.timer then state.timer:stop(); state.timer:close() end
  state.timer = vim.uv.new_timer()
  state.timer:start(options.recovery_delay, 0, vim.schedule_wrap(function()
    if alive(buf) and vim.bo[buf].modified then recover_local(buf, state); validate(buf, state) end
  end))
end
local function select(items, prompt, format, callback)
  vim.ui.select(items, { prompt = prompt, format_item = format }, function(item) if item then callback(item) end end)
end
local function show_list(title, rows, open)
  vim.cmd('botright vsplit')
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_win_set_buf(0, buf)
  vim.api.nvim_buf_set_name(buf, 'projman-view://' .. title .. '/' .. uuid())
  vim.bo[buf].buftype = 'nofile'; vim.bo[buf].bufhidden = 'wipe'; vim.bo[buf].swapfile = false
  local lines = {}
  for _, row in ipairs(rows) do table.insert(lines, row.text) end
  if #lines == 0 then lines = { '(empty)' } end
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  if open then vim.keymap.set('n', '<CR>', function() local row = rows[vim.api.nvim_win_get_cursor(0)[1]]; if row then open(row) end end, { buffer = buf, silent = true }) end
  return buf
end
local function pretty(value)
  local raw = vim.json.encode(value)
  local out, level, quoted, escaped = {}, 0, false, false
  local function newline() table.insert(out, '\n' .. string.rep('  ', level)) end
  for i = 1, #raw do
    local ch = raw:sub(i, i)
    if quoted then
      table.insert(out, ch)
      if escaped then escaped = false elseif ch == '\\' then escaped = true elseif ch == '"' then quoted = false end
    elseif ch == '"' then quoted = true; table.insert(out, ch)
    elseif ch == '{' or ch == '[' then table.insert(out, ch); level = level + 1; newline()
    elseif ch == '}' or ch == ']' then level = level - 1; newline(); table.insert(out, ch)
    elseif ch == ',' then table.insert(out, ch); newline()
    elseif ch == ':' then table.insert(out, ': ')
    else table.insert(out, ch) end
  end
  return table.concat(out)
end
function M.setup(opts)
  options = vim.tbl_extend('force', options, opts or {})
  local command = vim.deepcopy(options.command or require('projman.runtime').command())
  if options.workspace then vim.list_extend(command, { '--workspace', options.workspace }) end
  rpc.setup({ command = command, timeout = options.timeout or 10000, env = options.env })
  M.config = vim.tbl_extend('force', {}, options, { command = command })
  initialized = true
  require('projman.explorer').setup(options.explorer, function(id, win) M.open(id, nil, win) end)
  rpc.initialize(notify)
  local group = vim.api.nvim_create_augroup('ProjManLifecycle', { clear = true })
  vim.api.nvim_create_autocmd('VimLeavePre', { group = group, callback = function()
    for buf, state in pairs(M.buffers) do if alive(buf) and vim.bo[buf].modified then recover_local(buf, state) end end
    rpc.stop()
  end })
end
local function install(data, definition, is_new)
  local node = data.node or data
  local name = 'project://' .. node.id
  for buf, state in pairs(M.buffers) do if alive(buf) and state.node.id == node.id then vim.api.nvim_set_current_buf(buf); return buf end end
  local previous = vim.api.nvim_get_current_buf()
  table.insert(M.history, previous)
  local buf = vim.api.nvim_create_buf(true, false)
  vim.api.nvim_set_current_buf(buf)
  vim.api.nvim_buf_set_name(buf, name)
  vim.bo[buf].buftype = 'acwrite'; vim.bo[buf].bufhidden = 'hide'; vim.bo[buf].swapfile = false; vim.bo[buf].filetype = 'markdown'
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(data.text, '\n', { plain = true }))
  local state = { node = node, definition = definition, active_schema = data.active_schema_revision or node.schema_revision,
    staged = {}, generation = 0, expected_nodes = object(), recovery_id = uuid() }
  M.buffers[buf] = state
  state.expected_nodes[node.id] = node.revision
  vim.bo[buf].modified = is_new
  vim.api.nvim_create_autocmd('BufWriteCmd', { buffer = buf, callback = function() M.save(buf) end })
  vim.api.nvim_create_autocmd({ 'TextChanged', 'TextChangedI' }, { buffer = buf, callback = function() changed(buf, state) end })
  vim.api.nvim_create_autocmd('BufWipeout', { buffer = buf, callback = function()
    if vim.bo[buf].modified then recover_local(buf, state) end
    if state.timer then state.timer:stop(); state.timer:close() end
    M.buffers[buf] = nil
  end })
  if options.map_tab then
    for key, direction in pairs({ ['<Tab>'] = 1, ['<S-Tab>'] = -1 }) do
      local fallback = vim.fn.maparg(key, 'i', false, true)
      vim.keymap.set('i', key, function()
        local blink = package.loaded['blink.cmp']
        if blink and blink.is_menu_visible and blink.is_menu_visible() then
          vim.schedule(function() if direction == 1 then blink.select_next() else blink.select_prev() end end); return ''
        end
        if blink and blink.snippet_active and blink.snippet_active({ direction = direction }) then
          vim.schedule(function() if direction == 1 then blink.snippet_forward() else blink.snippet_backward() end end); return ''
        end
        local luasnip = package.loaded.luasnip
        if luasnip and luasnip.locally_jumpable(direction) then
          vim.schedule(function() luasnip.jump(direction) end); return ''
        end
        if vim.fn.pumvisible() == 1 then return direction == 1 and '<C-n>' or '<C-p>' end
        if vim.snippet and vim.snippet.active({ direction = direction }) then
          vim.schedule(function() vim.snippet.jump(direction) end); return ''
        end
        local cursor = vim.api.nvim_win_get_cursor(0)
        local inside = false
        for _, field in ipairs(fields(buf, state)) do if cursor[1] == field.line and cursor[2] >= field.column - 1 then inside = true end end
        if inside then vim.schedule(function() M.jump(direction) end); return '' end
        if fallback.callback then return fallback.callback() or '' end
        if fallback.rhs and fallback.rhs ~= '' then
          if fallback.expr == 1 then return vim.api.nvim_eval(fallback.rhs) end
          return fallback.rhs
        end
        return key
      end, { buffer = buf, expr = true, replace_keycodes = true, silent = true })
    end
  end
  _G.ProjManComplete = M.complete
  vim.bo[buf].omnifunc = 'v:lua.ProjManComplete'
  vim.keymap.set('n', 'gf', function() M.follow() end, { buffer = buf, silent = true })
  annotations(buf, state); validate(buf, state)
  if is_new then M.jump(1); recover_local(buf, state) end
  return buf
end
function M.open(id, callback, target_win)
  ready()
  rpc.request('node.document', { id = id }, function(err, data)
    if err then notify(err); if callback then callback(err) end; return end
    if target_win and not vim.api.nvim_win_is_valid(target_win) then
      if callback then callback({ code = 'cancelled', message = 'The editing window was closed' }) end
      return
    end
    local ok, buf = pcall(function()
      if target_win then return vim.api.nvim_win_call(target_win, function() return install(data, data.type, false) end) end
      return install(data, data.type, false)
    end)
    if not ok then local failure = { code = 'editor', message = tostring(buf) }; notify(failure); if callback then callback(failure) end; return end
    if callback then callback(nil, buf) end
  end)
end
function M.explore(root) ready(); return require('projman.explorer').open(root) end
function M.explorer_root() ready(); return require('projman.explorer').pick_root() end
function M.explorer_refresh() ready(); return require('projman.explorer').refresh() end
function M.explorer_toggle() ready(); return require('projman.explorer').toggle() end
function M.find(query)
  ready()
  local win = vim.api.nvim_get_current_win()
  return require('projman.picker').open({ query = query, on_select = function(item) M.open(item.id, nil, win) end })
end
function M.new(type_key, callback)
  ready()
  local function create(key)
    rpc.request('node.new', { type_key = key }, function(err, data)
      if err then notify(err); if callback then callback(err) end; return end
      rpc.request('type.show', { key = key }, function(type_err, definition)
        if type_err then notify(type_err); return end
        local buf = install(data, definition, true)
        if callback then callback(nil, buf) end
      end)
    end)
  end
  if type_key then create(type_key) else
    rpc.request('type.list', {}, function(err, data)
      if err then return notify(err) end
      select(data.node_types, 'Create node of type', function(t) return t.name .. ' (' .. t.key .. ')' end, function(t) create(t.key) end)
    end)
  end
end
local function entry_target(origin)
  if not vim.api.nvim_win_is_valid(origin) then return end
  local target = vim.api.nvim_win_call(origin, function() return require('projman.explorer').editing_window() end)
  local buf = vim.api.nvim_win_get_buf(target)
  if vim.bo[buf].modified and vim.bo[buf].bufhidden ~= 'hide' and not vim.o.hidden then
    vim.api.nvim_win_call(target, function() vim.cmd('rightbelow vsplit'); target = vim.api.nvim_get_current_win() end)
  end
  return target
end
local function notes(buf, win, insert)
  if not alive(buf) or not vim.api.nvim_win_is_valid(win) or vim.api.nvim_win_get_buf(win) ~= buf then return end
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  for line = 2, #lines do
    if lines[line] == '---' then
      if line == #lines then vim.api.nvim_buf_set_lines(buf, -1, -1, false, { '' }) end
      vim.api.nvim_win_call(win, function()
        vim.api.nvim_win_set_cursor(win, { line + 1, 0 }); vim.cmd('normal! zt')
      end)
      if insert and vim.api.nvim_get_current_win() == win then vim.cmd('startinsert') end
      return
    end
  end
end
function M.create(type_key, raw)
  ready()
  if raw or not options.guided_entry then return M.new(type_key) end
  if M.entry_busy then return notify('Finish or cancel the current entry first') end
  M.entry_busy = true
  local origin = vim.api.nvim_get_current_win()
  local function cancel(err)
    M.entry_busy = false
    if err then notify(err) else vim.notify('Node creation cancelled', vim.log.levels.INFO, { title = 'ProjMan' }) end
  end
  local function start(key)
    rpc.request('node.new', { type_key = key }, function(err, seed)
      if err then cancel(err); return end
      if not seed.type then cancel('Rebuild the Rust host with :Lazy build projman, then restart Neovim'); return end
      require('projman.entry').create(seed, { form = options.entry_mode == 'form' }, function(values, cancelled)
        if cancelled then cancel(); return end
        local win = entry_target(origin)
        if not win then cancel('The original editing window was closed'); return end
        local lines = { '--- projman' }
        for _, field in ipairs(seed.type.properties) do
          local value = values[field.key]; if value == nil then value = vim.NIL end
          lines[#lines + 1] = field.key .. ': ' .. vim.json.encode(value)
        end
        lines[#lines + 1] = '---'; lines[#lines + 1] = seed.body or ''
        seed.properties, seed.text = values, table.concat(lines, '\n')
        vim.api.nvim_set_current_win(win)
        local ok, buf = pcall(install, seed, seed.type, true)
        M.entry_busy = false
        if not ok then return notify(buf) end
        notes(buf, win, false)
        M.save(buf, function(save_err)
          if save_err then return end -- The completed local draft remains recoverable.
          if alive(buf) and not vim.bo[buf].modified then notes(buf, win, true) end
          local node = M.buffers[buf] and M.buffers[buf].node
          if node then vim.notify('Created ' .. seed.type.name .. (#node.missing > 0 and ' draft (required fields are still missing)' or '') .. '. Write notes; use <leader>Pv to edit properties.', vim.log.levels.INFO, { title = 'ProjMan' }) end
        end)
      end)
    end)
  end
  if type_key then start(type_key) else
    rpc.request('type.list', {}, function(err, data)
      if err then cancel(err.code == 'not_found' and 'Define a node type with :ProjManTypeNew before creating nodes' or err); return end
      if #data.node_types == 0 then cancel('Define a node type with :ProjManTypeNew before creating nodes'); return end
      vim.ui.select(data.node_types, { prompt = 'What would you like to create?', format_item = function(ty) return ty.name .. (ty.description and ty.description ~= '' and (' — ' .. ty.description) or '') end }, function(ty)
        if ty then start(ty.key) else cancel() end
      end)
    end)
  end
end
local function property_rows(buf)
  local rows, terminator = {}, nil
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  if lines[1] ~= '--- projman' then return nil, nil, 'The property header is damaged; restore its --- projman opening line' end
  for line = 2, #lines do
    if lines[line] == '---' then terminator = line; break end
    local key, raw = lines[line]:match('^%s*([a-z][a-z0-9_]*)%s*:%s*(.*)$')
    if key then
      if rows[key] then return nil, nil, 'Duplicate property ' .. key .. '; correct the header first' end
      rows[key] = { line = line, raw = raw }
    end
  end
  if not terminator then return nil, nil, 'The property header is missing its closing --- line' end
  return rows, terminator
end
local function field_value(field, raw)
  if raw == nil or vim.trim(raw) == '' or vim.trim(raw) == 'null' then return vim.NIL end
  local ok, value = pcall(vim.json.decode, raw)
  if field.type == 'string' or field.type == 'enum' or field.type == 'date' then
    return ok and type(value) == 'string' and value or vim.trim(raw)
  end
  if ok then return value end
  return raw
end
function M.properties(key, target_buf)
  local buf, state
  if target_buf then buf, state = target_buf, M.buffers[target_buf] else buf, state = current() end
  if not buf or not state or not alive(buf) then return end
  if state.saving or state.pending then return notify('Resolve the pending save before editing properties') end
  if state.entry_busy then return notify('Finish or cancel the current property entry first') end
  local entry = require('projman.entry')
  local function menu()
    if not alive(buf) then return end
    local rows, _, err = property_rows(buf); if err then notify(err); return end
    local values = object()
    for _, field in ipairs(state.definition.properties) do values[field.key] = field_value(field, rows[field.key] and rows[field.key].raw) end
    state.entry_busy = true
    local title = state.definition.name .. ' · ' .. vim.fn.strcharpart(entry.display(values[state.definition.display_property]), 0, 80)
    entry.choose_field(state.definition, values, title, nil, function(choice)
      state.entry_busy = false
      if not alive(buf) then return end
      if not choice or choice.close then return end
      if choice.save then
        local win = vim.fn.bufwinid(buf)
        M.save(buf, function(save_err) if not save_err and win ~= -1 then notes(buf, win, false) end end)
        return
      end
      M.properties(choice.field.key, buf)
    end)
  end
  if not key or key == '' then menu(); return end
  local field
  for _, item in ipairs(state.definition.properties) do if item.key == key then field = item; break end end
  if not field then return notify('Unknown property: ' .. key) end
  local rows, _, err = property_rows(buf); if err then return notify(err) end
  local original = rows[key] and rows[key].raw
  state.entry_busy = true
  entry.ask({ type_key = state.node.type_key, schema_revision = state.node.schema_revision }, field, field_value(field, original), state.definition.name, function(value, cancelled)
    state.entry_busy = false
    if cancelled or not alive(buf) then return end
    local latest, terminator, header_err = property_rows(buf)
    if header_err then return notify(header_err) end
    if (latest[key] and latest[key].raw) ~= original then return notify('This property changed while its prompt was open; the newer text has been preserved') end
    local line = latest[key] and latest[key].line or terminator
    vim.api.nvim_buf_set_lines(buf, line - 1, latest[key] and line or line - 1, false, { key .. ': ' .. vim.json.encode(value) })
    save_status(buf, state, 'modified', 'Property updated · :write saves this node')
    recover_local(buf, state); validate(buf, state)
    vim.schedule(function() if alive(buf) then menu() end end)
  end)
end
function M.jump(direction)
  local buf, state = current(); if not buf then return end
  local spans = fields(buf, state); if #spans == 0 then return end
  local cursor = vim.api.nvim_win_get_cursor(0)
  local index
  for i, span in ipairs(spans) do if span.line == cursor[1] then index = i end end
  if index then index = ((index - 1 + direction) % #spans) + 1 else
    index = direction == 1 and 1 or #spans
    if direction == 1 then for i, span in ipairs(spans) do if span.required then index = i; break end end end
  end
  vim.api.nvim_win_set_cursor(0, { spans[index].line, spans[index].column - 1 })
end
function M.complete(findstart, base)
  local buf, state = current(); if not buf then return findstart == 1 and -1 or {} end
  local line = vim.api.nvim_win_get_cursor(0)[1]
  for _, field in ipairs(fields(buf, state)) do
    if field.line == line then
      if findstart == 1 then return field.column - 1 end
      for _, definition in ipairs(state.definition.properties) do
        if definition.key == field.key then
          local values = {}
          for _, choice in ipairs(definition.choices or {}) do
            local encoded = vim.json.encode(choice)
            if encoded:sub(1, #base) == base then table.insert(values, encoded) end
          end
          return values
        end
      end
    end
  end
  return findstart == 1 and -1 or {}
end
local function apply_pending(buf, state, callback)
  local pending = state.pending
  state.saving = true
  save_status(buf, state, 'saving', 'Saving…')
  rpc.request('change.apply', pending.request, function(err, receipt)
    state.saving = false
    if not alive(buf) then return end
    if err then
      if err.code ~= 'unavailable' and err.code ~= 'io' and err.code ~= 'storage' then state.pending = nil end
      save_error(buf, state, err, pending.tick)
      if callback then callback(err) end
      return
    end
    state.node.revision = receipt.result.node_revisions[state.node.id] or state.node.revision
    state.node.schema_revision = receipt.schema_revision
    state.active_schema = receipt.schema_revision
    for id, revision in pairs(receipt.result.node_revisions) do state.expected_nodes[id] = revision end
    local remaining = {}
    for i = pending.staged_count + 1, #state.staged do table.insert(remaining, state.staged[i]) end
    state.staged = remaining
    state.node.properties = pending.properties
    state.node.body = pending.body
    state.node.missing = pending.missing
    if vim.api.nvim_buf_get_changedtick(buf) == pending.tick and state.generation == pending.generation then
      vim.bo[buf].modified = false
      rpc.request('recovery.remove', { id = state.recovery_id }, function() end)
    else recover_local(buf, state) end
    rpc.request('recovery.remove', { id = pending.recovery_id }, function() end)
    state.pending, state.error = nil, nil
    local message = 'Saved ' .. state.node.id:sub(1, 8) .. ' (revision ' .. state.node.revision .. ')'
    if vim.bo[buf].modified then message = message .. '; newer edits remain unsaved' end
    save_status(buf, state, vim.bo[buf].modified and 'modified' or 'saved', message)
    if options.notify_save then vim.notify(message, vim.log.levels.INFO, { title = 'ProjMan' }) end
    vim.api.nvim_exec_autocmds('User', { pattern = 'ProjManGraphChanged', modeline = false, data = { graph_revision = receipt.graph_revision } })
    validate(buf, state)
    if callback then callback(nil, receipt) end
  end)
end
function M.save(buf, callback)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = M.buffers[buf]; if not state then return end
  if state.saving then notify('A save is already in progress'); return end
  if state.pending then notify('A previous save has an unresolved outcome. Use :ProjManRetry or :ProjManConflict.'); return end
  state.saving = true
  state.error = nil
  save_status(buf, state, 'saving', 'Saving…')
  local snapshot, tick, generation = text(buf), vim.api.nvim_buf_get_changedtick(buf), state.generation
  local staged = vim.deepcopy(state.staged)
  rpc.request('document.parse', { type_key = state.node.type_key, schema_revision = state.node.schema_revision, text = snapshot }, function(err, parsed)
    if not alive(buf) then return end
    if err then save_error(buf, state, err, tick); recover_local(buf, state); if callback then callback(err) end; return end
    local op
    if state.node.revision == 0 then op = { op = 'create_node', id = state.node.id, type_key = state.node.type_key, properties = parsed.properties, body = parsed.body }
    else
      local unset = {}
      for key in pairs(state.node.properties or {}) do if parsed.properties[key] == nil then table.insert(unset, key) end end
      op = { op = 'update_node', id = state.node.id, set = parsed.properties, unset = unset, body = parsed.body }
    end
    local operations = { op }; vim.list_extend(operations, staged)
    local expected = vim.deepcopy(state.expected_nodes); expected[state.node.id] = state.node.revision
    local request = { operation_id = uuid(), expected_schema = state.active_schema, expected_nodes = expected, action = { kind = 'changes', operations = operations } }
    state.pending = { request = request, text = snapshot, properties = parsed.properties, body = parsed.body, missing = parsed.missing, tick = tick, generation = generation, staged_count = #staged, recovery_id = uuid() }
    local _, recovery_err = recovery.save(recovery_record(buf, state, state.pending))
    if recovery_err then
      state.saving = false; state.pending = nil
      local failure = { code = 'io', message = recovery_err }; save_error(buf, state, failure, tick); if callback then callback(failure) end; return
    end
    apply_pending(buf, state, callback)
  end)
end
function M.retry(callback)
  local buf, state = current(); if not buf then return end
  if not state.pending then return M.save(buf, callback) end
  if state.saving then return notify('A save is already in progress') end
  apply_pending(buf, state, callback)
end
function M.search(query)
  ready()
  rpc.request('search', { query = query or '', limit = 1000 }, function(err, data)
    if err then return notify(err) end
    select(data.items, 'Open node', function(n) return n.title .. ' [' .. n.type_key .. '] ' .. n.id end, function(n) M.open(n.id) end)
  end)
end
function M.follow()
  local line = vim.api.nvim_get_current_line()
  local id = line:match('%]%(%s*project://([%x%-]+)%)')
  if id then M.open(id) else vim.cmd('normal! gf') end
end
function M.back()
  while #M.history > 0 do local buf = table.remove(M.history); if alive(buf) then vim.api.nvim_set_current_buf(buf); return end end
end
function M.stage(operations, expected_nodes, buf)
  buf = buf or vim.api.nvim_get_current_buf(); local state = M.buffers[buf]
  if not state then return end
  vim.list_extend(state.staged, operations)
  for id, revision in pairs(expected_nodes) do
    if state.expected_nodes[id] == nil then state.expected_nodes[id] = revision end
  end
  state.generation = state.generation + 1
  vim.bo[buf].modified = true
  recover_local(buf, state)
end
function M.link(type_key)
  local buf, state = current(); if not buf then return end
  if state.node.revision == 0 then return notify('Save this new node before adding links') end
  rpc.request('node.list', { limit = 1000, relation = type_key, source = type_key and state.node.id or nil }, function(err, nodes)
    if err then return notify(err) end
    select(nodes.items, 'Link to node', function(n) return n.title .. ' [' .. n.type_key .. ']' end, function(target)
      rpc.request('link.suggest', { source = state.node.id, target = target.id }, function(suggest_err, suggestions)
        if suggest_err then return notify(suggest_err) end
        if type_key then suggestions.items = vim.tbl_filter(function(r) return r.type_key == type_key and r.direction == 'outgoing' end, suggestions.items) end
        select(suggestions.items, 'Relationship', function(r) return r.label .. ' (' .. r.direction .. ')' end, function(relation)
          rpc.request('export', {}, function(export_err, snapshot)
            if export_err then return notify(export_err) end
            local expected = object(); for id, node in pairs(snapshot.nodes) do expected[id] = node.revision end
            local function stage(props)
              M.stage({ { op = 'add_link', source = relation.source, target = relation.target, type_key = relation.type_key, properties = props } }, expected, buf)
            end
            if #relation.properties > 0 then
              vim.ui.input({ prompt = 'Relationship properties as JSON: ', default = '{}' }, function(input)
                if input then local ok, props = pcall(vim.json.decode, input); if ok then stage(props) else notify(props) end end
              end)
            else stage(object()) end
          end)
        end)
      end)
    end)
  end)
end
local function node_title(snapshot, id)
  local node = snapshot.nodes[id]
  if not node then return id end
  local schema = snapshot.schemas[tostring(node.schema_revision)] or snapshot.schemas[node.schema_revision]
  for _, definition in ipairs(schema.node_types) do
    if definition.key == node.type_key then
      local title = node.properties[definition.display_property]
      if type(title) == 'string' and title ~= '' then return title end
    end
  end
  return node.type_key .. ' ' .. id:sub(1, 8)
end
function M.links()
  local buf, state = current(); if not buf then return end
  rpc.request('export', {}, function(err, snapshot)
    if err then return notify(err) end
    local rows = {}
    for _, edge in pairs(snapshot.edges) do
      if edge.source == state.node.id or edge.target == state.node.id then
        local target = edge.source == state.node.id and edge.target or edge.source
        local schema = snapshot.schemas[tostring(edge.schema_revision)] or snapshot.schemas[edge.schema_revision]
        local label = edge.type_key
        for _, relation in ipairs(schema.relationship_types) do
          if relation.key == edge.type_key then label = edge.source == state.node.id and relation.name or relation.inverse_name end
        end
        table.insert(rows, { id = target, edge = edge, text = (edge.source == state.node.id and '-> ' or '<- ') .. label .. ' ' .. node_title(snapshot, target) .. ' [' .. target .. ']' })
      end
    end
    table.sort(rows, function(a, b) return a.edge.position < b.edge.position end)
    local view = show_list('relationships', rows, function(row) M.open(row.id) end)
    vim.keymap.set('n', 'dd', function()
      local row_number = vim.api.nvim_win_get_cursor(0)[1]
      local row = rows[row_number]; if not row or row.removed then return end
      row.removed = true
      local expected = object(); for id, node in pairs(snapshot.nodes) do expected[id] = node.revision end
      M.stage({ { op = 'remove_link', id = row.edge.id } }, expected, buf)
      vim.bo[view].modifiable = true
      vim.api.nvim_buf_set_lines(view, row_number - 1, row_number, false, { '[removal staged] ' .. row.text })
      vim.bo[view].modifiable = false
      vim.notify('Removal staged; write the node to apply')
    end, { buffer = view })
  end)
end
function M.outline(family)
  local _, state = current(); if not state then return end
  local function display(selected)
    rpc.request('outline', { id = state.node.id, family = selected }, function(err, tree)
      if err then return notify(err) end
      local rows = {}
      local function visit(n, depth)
        table.insert(rows, { id = n.id, text = string.rep('  ', depth) .. n.title .. ' [' .. n.type_key .. ']' })
        for _, child in ipairs(n.children) do visit(child, depth + 1) end
      end
      visit(tree, 0); show_list('outline', rows, function(row) M.open(row.id) end)
    end)
  end
  if family then display(family) else
    rpc.request('type.list', {}, function(err, data)
      if err then return notify(err) end
      local choices, seen = {}, {}
      for _, relation in ipairs(data.relationship_types) do
        local key = relation.family ~= '' and relation.family or relation.key
        if relation.ordered and not seen[key] then table.insert(choices, key); seen[key] = true end
      end
      select(choices, 'Outline family', tostring, display)
    end)
  end
end
function M.backlinks()
  local _, state = current(); if not state then return end
  rpc.request('backlinks', { id = state.node.id }, function(err, data)
    if err then return notify(err) end
    local rows = {}
    for _, edge in ipairs(data.edges) do table.insert(rows, { id = edge.node.id, text = edge.label .. ' ' .. edge.node.id }) end
    for _, node in ipairs(data.mentions) do table.insert(rows, { id = node.id, text = 'Mentioned by ' .. node.id }) end
    show_list('backlinks', rows, function(row) M.open(row.id) end)
  end)
end
function M.types(transform)
  ready()
  rpc.request('workspace.show', {}, function(err, workspace)
    if err then return notify(err) end
    local function open_schema(schema)
      if transform then transform(schema) end
      local buf = vim.api.nvim_create_buf(true, false); vim.api.nvim_set_current_buf(buf)
      vim.api.nvim_buf_set_name(buf, 'project-schema://' .. rpc.workspace .. '/' .. uuid())
      vim.bo[buf].buftype = 'acwrite'; vim.bo[buf].bufhidden = 'hide'; vim.bo[buf].filetype = 'json'
      vim.api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(pretty(schema), '\n', { plain = true }))
      vim.bo[buf].modified = transform ~= nil
      vim.api.nvim_create_autocmd('BufWriteCmd', { buffer = buf, callback = function()
        local tick = vim.api.nvim_buf_get_changedtick(buf)
        local ok, draft = pcall(vim.json.decode, text(buf)); if not ok then return notify(draft) end
        rpc.request('schema.preview', { schema = draft }, function(preview_err, preview)
          if preview_err then return notify(preview_err) end
          local function publish(retain)
            rpc.request('change.apply', { operation_id = uuid(), expected_schema = workspace.schema_revision, expected_graph = preview.graph_revision,
              action = { kind = 'publish_schema', schema = draft, retain_legacy = retain } }, function(save_err, receipt)
              if save_err then return notify(save_err) end
              workspace.schema_revision = receipt.schema_revision
              vim.api.nvim_exec_autocmds('User', { pattern = 'ProjManGraphChanged', modeline = false, data = { graph_revision = receipt.graph_revision } })
              if alive(buf) and vim.api.nvim_buf_get_changedtick(buf) == tick then vim.bo[buf].modified = false end
            end)
          end
          if #preview.impact == 0 then publish(false) else
            show_list('schema-impact', vim.tbl_map(function(impact) return { text = vim.json.encode(impact) } end, preview.impact))
            select({ 'Retain affected data under its old schema', 'Cancel' }, 'Publish schema change?', tostring, function(choice)
              if choice ~= 'Cancel' then publish(true) end
            end)
          end
        end)
      end })
    end
    if workspace.schema_revision == 0 then open_schema({ node_types = {}, relationship_types = {} }) else
      rpc.request('schema.export', {}, function(export_err, result) if export_err then notify(export_err) else open_schema(result.schema) end end)
    end
  end)
end
function M.type_new(relation)
  vim.ui.input({ prompt = relation and 'Relationship stable key: ' or 'Node type stable key: ' }, function(key)
    if not key or key == '' then return end
    M.types(function(schema)
      if relation then
        table.insert(schema.relationship_types, { key = key, name = key, inverse_name = 'Inverse of ' .. key, sources = {}, targets = {} })
      else
        table.insert(schema.node_types, { key = key, name = key, display_property = 'name', properties = { { key = 'name', type = 'string', required = true } } })
      end
    end)
  end)
end
function M.conflict()
  local buf, state = current(); if not buf then return end
  rpc.request('node.document', { id = state.node.id }, function(err, data)
    if err then return notify(err) end
    local rows = {}; for _, line in ipairs(vim.split(data.text, '\n', { plain = true })) do table.insert(rows, { text = line }) end
    local view = show_list('current-saved-revision-' .. data.node.revision, rows)
    vim.bo[view].filetype = 'markdown'
    vim.cmd('diffthis')
    local win = vim.fn.bufwinid(buf); if win ~= -1 then vim.api.nvim_win_call(win, function() vim.cmd('diffthis') end) end
    vim.notify('Your buffer is preserved. Compare this saved revision before reconciling; CLI recovery exports both base revision and local text.')
  end)
end
function M.recover()
  ready()
  rpc.request('recovery.list', {}, function(err, data)
    if err then return notify(err) end
    select(data.items, 'Restore recovery through revision checks', function(r) return r.node_id .. ' revision ' .. r.expected_revision .. ' — ' .. r.id end, function(record)
      rpc.request('recovery.restore', { id = record.id }, function(restore_err)
        if restore_err then return notify(restore_err) end
        M.open(record.node_id)
      end)
    end)
  end)
end
function M.unstage()
  local buf, state = current(); if not buf then return end
  if state.saving or state.pending then return notify('Resolve the pending save with :ProjManRetry first') end
  state.staged = {}; state.expected_nodes = object(); state.expected_nodes[state.node.id] = state.node.revision
  state.generation = state.generation + 1; vim.bo[buf].modified = true
  recover_local(buf, state)
  vim.notify('Staged relationship changes cleared; node text is preserved')
end
function M.reparent(edge_id, parent_id)
  local buf, state = current(); if not buf then return end
  rpc.request('export', {}, function(err, snapshot)
    if err then return notify(err) end
    local edge = snapshot.edges[edge_id]
    if not edge then return notify('Relationship does not exist') end
    local expected = object(); for id, node in pairs(snapshot.nodes) do expected[id] = node.revision end
    M.stage({ { op = 'remove_link', id = edge_id }, { op = 'add_link', id = edge_id, source = parent_id,
      target = edge.target, type_key = edge.type_key, properties = edge.properties } }, expected, buf)
    vim.notify('Move staged; write the node to apply')
  end)
end
function M.reorder(family, edge_ids)
  local buf, state = current(); if not buf then return end
  rpc.request('export', {}, function(err, snapshot)
    if err then return notify(err) end
    local expected = object(); for id, node in pairs(snapshot.nodes) do expected[id] = node.revision end
    M.stage({ { op = 'reorder', parent = state.node.id, family = family, edge_ids = edge_ids } }, expected, buf)
  end)
end
function M.reconcile()
  local buf, state = current(); if not buf then return end
  if state.saving then return notify('Wait for the pending save first') end
  M.conflict()
  rpc.request('node.document', { id = state.node.id }, function(err, data)
    if err then return notify(err) end
    if data.node.schema_revision ~= state.node.schema_revision then return notify('Schema changed; export the recovery and migrate explicitly through the CLI') end
    select({ 'Keep my edited text and rebase on revision ' .. data.node.revision, 'Cancel' },
      'After comparing the saved version, choose how to proceed', tostring, function(choice)
        if choice == 'Cancel' then return end
        -- Retain the previous pending recovery for audit; do not automatically reapply it.
        state.pending, state.error = nil, nil
        state.node = data.node; state.active_schema = data.active_schema_revision
        state.expected_nodes[state.node.id] = state.node.revision
        vim.bo[buf].modified = true
        recover_local(buf, state)
        vim.notify('Local text preserved against the reviewed revision. Write to validate and save.')
      end)
  end)
end
local function review_text(data)
  if not data.diff then return pretty(data) end
  local lines = { 'Proposal ' .. data.proposal.draft.id, 'Digest ' .. data.proposal.digest, '' }
  for _, group in ipairs(data.proposal.draft.groups) do
    table.insert(lines, group.id .. ': ' .. group.title)
    table.insert(lines, '  ' .. group.rationale)
    if #group.depends_on > 0 then table.insert(lines, '  Requires: ' .. table.concat(group.depends_on, ', ')) end
  end
  for _, question in ipairs(data.proposal.draft.questions) do table.insert(lines, 'Question: ' .. question) end
  table.insert(lines, '')
  table.insert(lines, data.diff)
  table.insert(lines, 'a: review selected groups and apply    r: reject')
  return table.concat(lines, '\n')
end
function M.proposals()
  ready()
  rpc.request('proposal.list', {}, function(err, data)
    if err then return notify(err) end
    select(data.items, 'Review proposal', function(p) return p.id .. ' [' .. p.status .. ']' end, function(p) M.review(p.id) end)
  end)
end
function M.review(id)
  ready()
  rpc.request('proposal.show', { id = id }, function(err, data)
    if err then return notify(err) end
    local rows = {}; for _, line in ipairs(vim.split(review_text(data), '\n', { plain = true })) do table.insert(rows, { text = line }) end
    local view = show_list('proposal-review', rows)
    vim.bo[view].filetype = 'diff'
    local proposal = data.proposal
    local function apply_action(action)
      rpc.request('workspace.show', {}, function(workspace_err, workspace)
        if workspace_err then return notify(workspace_err) end
        rpc.request('change.apply', { operation_id = uuid(), expected_schema = workspace.schema_revision, action = action }, function(change_err, receipt)
          if change_err then return notify(change_err) end
          vim.api.nvim_exec_autocmds('User', { pattern = 'ProjManGraphChanged', modeline = false, data = { graph_revision = receipt.graph_revision } })
          vim.notify('Proposal ' .. (action.kind == 'apply_proposal' and 'applied' or 'rejected'))
        end)
      end)
    end
    vim.keymap.set('n', 'r', function()
      apply_action({ kind = 'reject_proposal', id = id, digest = proposal.digest })
    end, { buffer = view, desc = 'Reject proposal' })
    vim.keymap.set('n', 'a', function()
      local defaults = {}; for _, group in ipairs(proposal.draft.groups) do table.insert(defaults, group.id) end
      vim.ui.input({ prompt = 'Groups to review and apply (comma separated): ', default = table.concat(defaults, ',') }, function(input)
        if not input then return end
        local groups = vim.split(input, ',', { trimempty = true })
        for i, group in ipairs(groups) do groups[i] = vim.trim(group) end
        rpc.request('proposal.validate', { id = id, groups = groups }, function(validation_err, preview)
          if validation_err then return notify(validation_err) end
          local selected_rows = {}; for _, line in ipairs(vim.split(review_text(preview), '\n', { plain = true })) do table.insert(selected_rows, { text = line }) end
          show_list('selected-proposal-changes', selected_rows)
          select({ 'Apply these reviewed groups', 'Cancel' }, 'Apply exactly ' .. table.concat(groups, ', ') .. '?', tostring, function(choice)
            if choice ~= 'Cancel' then apply_action({ kind = 'apply_proposal', id = id, digest = proposal.digest, groups = groups }) end
          end)
        end)
      end)
    end, { buffer = view, desc = 'Review and apply selected groups' })
  end)
end
return M
