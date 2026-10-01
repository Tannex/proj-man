local root = assert(vim.env.PROJMAN_TEST_ROOT)
local lazy_root = assert(vim.env.PROJMAN_TEST_LAZY_ROOT)
local temp = assert(vim.env.XDG_STATE_HOME)
vim.g.mapleader = ' '
vim.opt.loadplugins = true
vim.opt.runtimepath:prepend(lazy_root .. '/lazy.nvim')
local specs = dofile(vim.env.PROJMAN_TEST_SPEC or (root .. '/nvim/lazyvim.lua'))
local plugin_spec = specs[1]
assert(plugin_spec.dir == root .. '/nvim' and plugin_spec.lazy and plugin_spec.main == 'projman')
specs[#specs + 1] = { name = 'which-key.nvim', dir = lazy_root .. '/which-key.nvim', lazy = true, opts = {} }
require('lazy').setup({ spec = specs, root = temp .. '/plugins', lockfile = temp .. '/lazy-lock.json',
  install = { missing = false }, checker = { enabled = false }, change_detection = { enabled = false, notify = false },
  performance = { rtp = { reset = false } } })
assert(not package.loaded.projman, 'Plugin must stay unloaded until an action is used')
local shortcut = vim.fn.maparg('<leader>Pe', 'n', false, true)
assert(shortcut.desc == 'ProjMan explorer', 'Lazy-loading shortcut is discoverable')
-- A command loads the plugin without needing a database connection.
vim.cmd('ProjManBack')
assert(package.loaded.projman, 'Command triggers lazy loading')
local plugin = require('projman')
assert(vim.fn.executable(plugin.config.command[1]) == 1, 'Built executable is discovered automatically')
local expected = {}; for _, command in ipairs(plugin_spec.cmd) do expected[command] = true; assert(vim.fn.exists(':' .. command) == 2, command) end
for command in pairs(vim.api.nvim_get_commands({})) do if command:match('^ProjMan') then assert(expected[command], 'Command omitted from lazy spec: ' .. command) end end
require('which-key').setup(require('lazy.core.plugin').values(require('lazy.core.config').plugins['which-key.nvim'], 'opts', false))
local groups = require('which-key.config').options.spec
assert(vim.iter(groups):any(function(group) return group[1] == '<leader>P' and group.group == 'ProjMan' end), 'which-key contains ProjMan group')
assert(vim.wait(3000, function() return require('projman.rpc').workspace ~= nil end, 10), 'Host initializes')
require('projman.rpc').stop()
print('LazyVim integration checks passed: lazy loading, shortcuts, command coverage, executable discovery, which-key group')
vim.cmd('qa!')
