local root = assert(vim.env.PROJMAN_TEST_ROOT)
local notifications = {}
vim.notify = function(message, level) notifications[#notifications + 1] = { message = message, level = level } end
if vim.env.PROJMAN_TEST_LAZY_ROOT then
  vim.opt.loadplugins = true
  vim.g.mapleader = ' '
  vim.opt.runtimepath:prepend(vim.env.PROJMAN_TEST_LAZY_ROOT .. '/lazy.nvim')
  local specs = dofile(root .. '/nvim/lazyvim.lua')
  -- Use the installed spec's automatic executable lookup in lazy.nvim mode.
  require('lazy').setup({ spec = specs, root = vim.env.XDG_STATE_HOME .. '/plugins', lockfile = vim.env.XDG_STATE_HOME .. '/lazy-lock.json',
    install = { missing = false }, checker = { enabled = false }, change_detection = { enabled = false, notify = false },
    performance = { rtp = { reset = false } } })
  vim.cmd('ProjManBack')
else
  vim.opt.runtimepath:prepend(root .. '/nvim')
  require('projman').setup({ command = { root .. '/target/debug/projman' } })
end
local plugin, rpc = require('projman'), require('projman.rpc')
local function await(predicate) assert(vim.wait(10000, predicate, 10), 'Timed out') end
local function request(method, params)
  local done, err, value = false, nil, nil
  rpc.request(method, params, function(e, v) done, err, value = true, e, v end)
  await(function() return done end)
  assert(not err, vim.inspect(err)); return value
end
local opened, buf = false, nil
plugin.open(vim.env.PROJMAN_TEST_NODE, function(err, result) assert(not err, vim.inspect(err)); buf, opened = result, true end)
await(function() return opened end)
local state = plugin.buffers[buf]
local function write()
  vim.cmd('write') -- Exercise the command and its BufWriteCmd handler, not M.save.
  assert(state.save_status.phase == 'saving')
  await(function() return not state.saving end)
end
local function saved()
  assert(not state.error, vim.inspect(state.error))
  assert(state.save_status.phase == 'saved' and not vim.bo[buf].modified)
  assert(vim.b[buf].projman_save_status.message:find('revision', 1, true))
end
vim.api.nvim_buf_set_lines(buf, 0, -1, false, {
  '--- projman', 'name: Updated task: café', 'status: in progress', 'due: 2026-10-01',
  'hours: 4', 'ready: false', 'tags: ["UI", "graph"]', '---', '## Notes', '', 'Body text with --- markers.', '',
})
write(); saved()
local node = request('node.get', { id = state.node.id })
assert(node.properties.name == 'Updated task: café' and node.properties.status == 'in progress')
assert(node.properties.hours == 4 and node.properties.due == '2026-10-01')
assert(node.body == '## Notes\n\nBody text with --- markers.\n')
assert(state.node.body == node.body)
assert(notifications[#notifications].message:find('Saved ', 1, true))
local revision = node.revision
vim.api.nvim_buf_set_lines(buf, 4, 5, false, { 'hours: three' })
write()
assert(state.error and state.error.details.field == 'hours' and state.error.details.line == 5)
assert(state.save_status.phase == 'error' and state.save_status.message:find('Not saved:', 1, true))
assert(vim.bo[buf].modified)
assert(request('node.get', { id = state.node.id }).revision == revision)
local diagnostics = vim.diagnostic.get(buf, { namespace = vim.api.nvim_create_namespace('projman') })
assert(#diagnostics == 1 and diagnostics[1].lnum == 4, 'Save error must point at the actual property line')
assert(diagnostics[1].message:find('hours', 1, true))
vim.api.nvim_buf_set_lines(buf, 4, 5, false, { 'hours: 6' })
write(); saved()
assert(request('node.get', { id = state.node.id }).properties.hours == 6)
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'name: ' })
write(); saved()
assert(state.node.missing[1] == 'name', 'A blank required field is an incomplete saved draft')
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'name: "Final quoted value"' })
write(); saved()
-- New nodes must also accept plain text through the real :write command.
opened = false
plugin.new('task', function(err, result) assert(not err, vim.inspect(err)); buf, opened = result, true end)
await(function() return opened end)
state = plugin.buffers[buf]
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'name: New task without quotes' })
write(); saved()
assert(request('node.get', { id = state.node.id }).properties.name == 'New task without quotes')
print('Actual :write checks passed: plain text, enums/dates, typed validation, exact diagnostics, drafts and save acknowledgement')
rpc.stop();vim.cmd('qa!')
