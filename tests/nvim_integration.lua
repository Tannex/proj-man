local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
vim.cmd('runtime plugin/projman.lua')
local errors = {}
vim.notify = function(message, level) if level == vim.log.levels.ERROR then table.insert(errors, message) end end
vim.keymap.set('i', '<Tab>', function() return 'FALLBACK' end, { expr = true })
local plugin = require('projman')
local rpc = require('projman.rpc')
plugin.setup({ command = { root .. '/target/debug/projman' }, recovery_delay = 30 })
local function await(operation)
  local done, failure, value = false, nil, nil
  operation(function(err, result) failure, value, done = err, result, true end)
  assert(vim.wait(10000, function() return done end, 10), 'Timed out waiting for operation')
  return failure, value
end
local function call(method, params)
  local err, value = await(function(cb) rpc.request(method, params, cb) end)
  assert(not err, err and vim.inspect(err))
  return value
end
local node = assert(vim.env.PROJMAN_TEST_NODE)
local err, buf = await(function(cb) plugin.open(node, cb) end)
assert(not err, vim.inspect(err))
assert(vim.bo[buf].buftype == 'acwrite')
assert(vim.api.nvim_buf_get_name(buf) == 'project://' .. node)
local state = plugin.buffers[buf]
vim.api.nvim_win_set_cursor(0, { 1, 0 })
plugin.jump(1)
assert(vim.api.nvim_win_get_cursor(0)[1] == 2, 'First required property is focused')
plugin.jump(1)
assert(vim.api.nvim_win_get_cursor(0)[1] == 3, 'Tab order follows schema')
plugin.jump(-1)
assert(vim.api.nvim_win_get_cursor(0)[1] == 2)
local function keys(value)
  vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(value, true, false, true), 'xt', false)
end
keys('i<Tab><Esc>')
assert(vim.wait(1000, function() return vim.api.nvim_win_get_cursor(0)[1] == 3 end, 10), 'Actual Tab mapping moves to next property')
keys('i<S-Tab><Esc>')
assert(vim.wait(1000, function() return vim.api.nvim_win_get_cursor(0)[1] == 2 end, 10), 'Actual Shift-Tab mapping moves back')
vim.api.nvim_win_set_cursor(0, { 3, 10 })
assert(plugin.complete(0, '"h')[1] == '"high"', 'Enum completion comes from schema')
local last = vim.api.nvim_buf_line_count(buf)
vim.api.nvim_win_set_cursor(0, { last, 0 })
keys('i<Tab><Esc>')
assert(vim.api.nvim_buf_get_lines(buf, last - 1, last, false)[1]:find('FALLBACK', 1, true), 'Tab outside header preserves existing mapping')
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'summary: "Saved snapshot"' })
local done, save_err = false, nil
plugin.save(buf, function(e) save_err, done = e, true end)
-- The request captures the snapshot before the editor changes again.
vim.api.nvim_buf_set_lines(buf, -1, -1, false, { 'Typed while saving.' })
assert(vim.wait(10000, function() return done end, 10))
assert(not save_err, vim.inspect(save_err))
assert(vim.bo[buf].modified, 'Newer edits must remain dirty')
assert(table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), '\n'):find('Typed while saving.', 1, true))
local saved = call('node.get', { id = node })
assert(saved.properties.summary == 'Saved snapshot')
assert(not saved.body:find('Typed while saving.', 1, true), 'Only the original snapshot was saved')
err = await(function(cb) plugin.save(buf, cb) end)
assert(not err, vim.inspect(err))
assert(not vim.bo[buf].modified)
assert(call('node.get', { id = node }).body:find('Typed while saving.', 1, true))
-- A separate writer changes the node. The editor must preserve its conflicting text.
local request = {
  operation_id = 'bd877eb6-aab2-4a08-8442-c4f2156d81c0', expected_schema = 1,
  expected_nodes = { [node] = state.node.revision },
  action = { kind = 'changes', operations = { { op = 'update_node', id = node, set = { summary = 'External writer' } } } },
}
call('change.apply', request)
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'summary: "Local conflicting work"' })
err = await(function(cb) plugin.save(buf, cb) end)
assert(err and err.code == 'conflict', 'Stale editor save must report a conflict')
assert(vim.bo[buf].modified)
assert(vim.api.nvim_buf_get_lines(buf, 1, 2, false)[1] == 'summary: "Local conflicting work"')
assert(call('node.get', { id = node }).properties.summary == 'External writer')
assert(#call('recovery.list', {}).items > 0, 'Failed save has durable recovery')
-- Creation also uses the schema, including missing required values and defaults.
err, buf = await(function(cb) plugin.new('deliverable', cb) end)
assert(not err, vim.inspect(err))
assert(plugin.buffers[buf].definition.key == 'deliverable')
err = await(function(cb) plugin.save(buf, cb) end)
assert(not err, vim.inspect(err))
local created = call('node.get', { id = plugin.buffers[buf].node.id })
assert(created.missing[1] == 'label')
assert(created.properties.accepted == false)
-- Schema buffers also publish through the shared core.
plugin.types()
assert(vim.wait(10000, function() return vim.api.nvim_buf_get_name(0):find('project-schema://', 1, true) ~= nil end, 10))
local schema_buf = vim.api.nvim_get_current_buf()
local schema_lines = vim.api.nvim_buf_get_lines(schema_buf, 0, -1, false)
local schema_text = table.concat(schema_lines, '\n'):gsub('"Investigation"', '"Research question"')
vim.api.nvim_buf_set_lines(schema_buf, 0, -1, false, vim.split(schema_text, '\n', { plain = true }))
vim.cmd('write')
assert(vim.wait(10000, function() return not vim.bo[schema_buf].modified end, 10), 'Schema save completes')
assert(call('type.show', { key = 'investigation' }).name == 'Research question')
-- Stage relationships in the editor, then commit them with the node.
err, buf = await(function(cb) plugin.new('investigation', cb) end)
assert(not err, vim.inspect(err))
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'summary: "Editor graph parent"' })
err = await(function(cb) plugin.save(buf, cb) end)
assert(not err, vim.inspect(err))
local parent = plugin.buffers[buf].node.id
local snapshot = call('export', {})
local expected = vim.empty_dict(); for id, item in pairs(snapshot.nodes) do expected[id] = item.revision end
plugin.stage({ { op = 'add_link', source = parent, target = created.id, type_key = 'contains' } }, expected, buf)
assert(#call('link.list', { id = parent }).items == 0, 'Staging does not mutate graph')
err = await(function(cb) plugin.save(buf, cb) end)
assert(not err, vim.inspect(err))
local edge = call('link.list', { id = parent }).items[1]
assert(edge.target == created.id)
plugin.links()
assert(vim.wait(10000, function() return vim.api.nvim_buf_get_name(0):find('projman-view://relationships', 1, true) ~= nil end, 10))
assert(vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]:find('Contains', 1, true))
vim.api.nvim_set_current_buf(buf)
plugin.outline('breakdown')
assert(vim.wait(10000, function() return vim.api.nvim_buf_get_name(0):find('projman-view://outline', 1, true) ~= nil end, 10))
assert(vim.api.nvim_buf_line_count(0) == 2)
vim.api.nvim_set_current_buf(buf)
local candidate = call('node.new', { type_key = 'investigation' })
snapshot = call('export', {})
call('change.apply', { operation_id = '2f7f2d30-8dba-4d41-9618-7ec5261fa7c1', expected_schema = snapshot.schema_revision,
  action = { kind = 'changes', operations = { { op = 'create_node', id = candidate.id, type_key = 'investigation' } } } })
plugin.reparent(edge.id, candidate.id)
assert(vim.wait(10000, function() return #plugin.buffers[buf].staged == 2 end, 10))
err = await(function(cb) plugin.save(buf, cb) end)
assert(not err, vim.inspect(err))
assert(#call('outline', { id = parent, family = 'breakdown' }).children == 0)
assert(call('outline', { id = candidate.id, family = 'breakdown' }).children[1].id == created.id)
-- Review and explicitly approve a manual proposal using the registered UI action.
snapshot = call('export', {})
local proposal_id = '2cb988ec-5936-48b8-966a-35cfbc6f02eb'
call('change.apply', { operation_id = 'f8ce858c-f2ce-4d47-b7f3-4f303ffbf0f7', expected_schema = snapshot.schema_revision,
  action = { kind = 'submit_proposal', proposal = { id = proposal_id, base_schema = snapshot.schema_revision, base_graph = snapshot.revision,
    expected_nodes = { [parent] = snapshot.nodes[parent].revision }, groups = { { id = 'note', title = 'Document result', rationale = 'Reviewed clarification',
      operations = { { op = 'update_node', id = parent, body = 'Explicitly approved in editor review.' } } } } } } })
plugin.review(proposal_id)
assert(vim.wait(10000, function() return vim.api.nvim_buf_get_name(0):find('projman-view://proposal-review', 1, true) ~= nil end, 10))
local old_input, old_select, old_request = vim.ui.input, vim.ui.select, rpc.request
local applied, approval_error = false, nil
vim.ui.input = function(_, cb) cb('note') end
vim.ui.select = function(items, _, cb) cb(items[1]) end
rpc.request = function(method, params, cb)
  if method == 'change.apply' and params.action.kind == 'apply_proposal' then
    return old_request(method, params, function(e, result) approval_error, applied = e, true; cb(e, result) end)
  end
  return old_request(method, params, cb)
end
local accept_mapping = vim.fn.maparg('a', 'n', false, true)
assert(type(accept_mapping.callback) == 'function')
accept_mapping.callback()
assert(vim.wait(10000, function() return applied end, 10))
assert(not approval_error, vim.inspect(approval_error))
vim.ui.input, vim.ui.select, rpc.request = old_input, old_select, old_request
assert(call('node.get', { id = parent }).body == 'Explicitly approved in editor review.')
print('Neovim integration checks passed: Tab mappings, completion, editing, late edits, conflicts, recovery, schemas, staged links, reparenting, outline, proposal review')
rpc.stop()
vim.cmd('qa!')
