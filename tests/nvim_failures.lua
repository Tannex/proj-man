local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
vim.notify = function() end
local plugin, rpc = require('projman'), require('projman.rpc')
plugin.setup({ command = { 'python3', root .. '/tests/slow_host.py', root .. '/target/debug/projman' }, timeout = 2000, recovery_delay = 10 })
local function await(operation)
  local done, error, result = false, nil, nil
  operation(function(e, r) error, result, done = e, r, true end)
  assert(vim.wait(10000, function() return done end, 10), 'Timed out')
  return error, result
end
local err, buf = await(function(cb) plugin.open(vim.env.PROJMAN_TEST_NODE, cb) end)
assert(not err, vim.inspect(err))
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'summary: "Survives host failure"' })
local done, save_error = false, nil
plugin.save(buf, function(e) save_error, done = e, true end)
assert(vim.wait(2000, function() return plugin.buffers[buf].pending ~= nil end, 1))
rpc.stop()
assert(vim.wait(5000, function() return done end, 10))
assert(save_error and save_error.code == 'unavailable')
assert(vim.bo[buf].modified)
assert(vim.api.nvim_buf_get_lines(buf, 1, 2, false)[1] == 'summary: "Survives host failure"')
local pending = plugin.buffers[buf].pending
assert(pending, 'Unacknowledged mutation is retained for retry')
local state_dir = vim.env.XDG_STATE_HOME .. '/projman/recovery/'
assert(vim.fn.filereadable(state_dir .. pending.recovery_id .. '.json') == 1)
err = await(function(cb) plugin.retry(cb) end)
assert(not err, vim.inspect(err))
assert(not vim.bo[buf].modified)
err = await(function(cb) rpc.request('operation.get', { id = pending.request.operation_id }, cb) end)
assert(not err, 'Retried request has a persisted receipt')
print('Neovim host failure checks passed: local recovery, preserved edits, reconnect, exact-request retry')
rpc.stop()
vim.cmd('qa!')
