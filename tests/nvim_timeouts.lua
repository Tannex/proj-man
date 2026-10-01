local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
vim.notify = function() end
local plugin, rpc = require('projman'), require('projman.rpc')
plugin.setup({ command = { 'python3', root .. '/tests/slow_host.py', root .. '/target/debug/projman' }, timeout = 2000 })
local function await(operation)
  local done, err, value = false, nil, nil
  operation(function(e, r) err, value, done = e, r, true end)
  assert(vim.wait(10000, function() return done end, 10))
  return err, value
end
local err, buf = await(function(cb) plugin.open(vim.env.PROJMAN_TEST_NODE, cb) end)
assert(not err, vim.inspect(err))
local before = plugin.buffers[buf].node.revision
-- Let initial diagnostics finish, then use a timeout shorter than mutation delivery.
vim.wait(100, function() return false end, 10)
rpc.setup({ timeout = 140 })
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'summary: "Unknown outcome recovered once"' })
err = await(function(cb) plugin.save(buf, cb) end)
assert(err and err.code == 'unavailable', 'Delayed mutation should time out')
assert(plugin.buffers[buf].pending)
assert(vim.bo[buf].modified)
rpc.setup({ timeout = 2000 })
err = await(function(cb) plugin.retry(cb) end)
assert(not err, vim.inspect(err))
assert(plugin.buffers[buf].node.revision == before + 1, 'Lost response retry must not apply twice')
assert(not vim.bo[buf].modified)
print('Neovim timeout checks passed: unknown outcome, persisted request, single application after retry')
rpc.stop()
vim.cmd('qa!')
