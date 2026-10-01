local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
local plugin = require('projman')
plugin.setup({ command = { root .. '/target/debug/projman' } })
local done, failure, buf = false, nil, nil
plugin.open(vim.env.PROJMAN_TEST_NODE, function(err, result) failure, buf, done = err, result, true end)
assert(vim.wait(10000, function() return done end, 10))
assert(not failure, vim.inspect(failure))
vim.api.nvim_buf_set_lines(buf, 1, 2, false, { 'summary: "Pending quit save"' })
local ok, err = pcall(vim.cmd, 'wq')
-- If :wq refuses until persistence finishes, wait and close normally.
assert(vim.wait(10000, function() return not plugin.buffers[buf].saving end, 10))
assert(not vim.bo[buf].modified, tostring(err))
vim.cmd('qa!')
