local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
vim.cmd('runtime plugin/projman.lua')
local ids = vim.json.decode(table.concat(vim.fn.readfile(vim.env.PROJMAN_TEST_IDS), '\n'))
local plugin, explorer, picker, rpc = require('projman'), require('projman.explorer'), require('projman.picker'), require('projman.rpc')
plugin.setup({}) -- Exercise the same automatic executable lookup used by LazyVim.
local function await(predicate) assert(vim.wait(10000, predicate, 5), 'Timed out') end
local function map(key, mode) vim.fn.maparg(key, mode or 'n', false, true).callback() end
local done, edit_buf
plugin.open(ids.root, function(err, buf) assert(not err, vim.inspect(err)); edit_buf=buf;done=true end)
await(function() return done end)
-- Completion/snippet integrations keep precedence over field navigation.
vim.api.nvim_win_set_cursor(0, { 2, 8 })
local blink_before, luasnip_before = package.loaded['blink.cmp'], package.loaded.luasnip
local completion_selected = false
package.loaded['blink.cmp'] = { is_menu_visible = function() return true end, select_next = function() completion_selected = true end }
map('<Tab>', 'i'); await(function() return completion_selected end)
local snippet_jumped = false
package.loaded['blink.cmp'] = nil
package.loaded.luasnip = { locally_jumpable = function() return true end, jump = function() snippet_jumped = true end }
map('<Tab>', 'i'); await(function() return snippet_jumped end)
package.loaded['blink.cmp'], package.loaded.luasnip = blink_before, luasnip_before
vim.api.nvim_buf_set_lines(edit_buf,-1,-1,false,{'Unsaved notes must survive exploring'})
local original_win=vim.api.nvim_get_current_win()
local p=plugin.explore()
await(function() return not p.loading end)
assert(#p.items==50 and p.total==71)
vim.api.nvim_buf_set_lines(p.input_buf,0,-1,false,{'needle topic'})
vim.api.nvim_exec_autocmds('TextChangedI',{buffer=p.input_buf})
await(function() return not p.loading end)
assert(#p.items==1 and p.items[1].id==ids.needle)
map('<CR>','i')
local s=explorer.state();await(function() return not s.loading end)
assert(s.root==ids.needle and #s.data.nodes==1)
plugin.explore(ids.root);await(function() return not s.loading end)
assert(s.nodes[ids.d].parent==ids.b and s.nodes[ids.d].depth==2)
local function select(id,reference)
  for line,row in pairs(s.rows) do if row.id==id and not not row.reference==not not reference then
    vim.api.nvim_set_current_win(s.win);vim.api.nvim_win_set_cursor(s.win,{line,0});return
  end end
  error('Missing row '..id)
end
select(ids.b);map('l');select(ids.d)
-- Slow down the document response and move focus back to the sidebar while waiting.
local request=rpc.request
rpc.request=function(method,params,callback)
  if method=='node.document' then return request(method,params,function(err,data) vim.defer_fn(function() callback(err,data) end,40) end) end
  return request(method,params,callback)
end
map('<CR>');vim.api.nvim_set_current_win(s.win)
await(function() return vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(original_win))=='project://'..ids.d end)
rpc.request=request
assert(vim.api.nvim_win_get_buf(s.win)==s.buf and vim.api.nvim_get_current_win()==s.win)
assert(vim.bo[edit_buf].modified and table.concat(vim.api.nvim_buf_get_lines(edit_buf,0,-1,false),'\n'):find('Unsaved notes',1,true))
select(ids.a);map('l');select(ids.c);map('l');select(ids.d,true);map('gp')
assert(not s.rows[vim.api.nvim_win_get_cursor(s.win)[1]].reference)
map('d');await(function() return not s.loading end);assert(s.options.direction=='incoming')
map('d');await(function() return not s.loading end);assert(s.options.direction=='both' and s.nodes[ids.d].depth==1)
select(ids.d);map('R');await(function() return not s.loading end);assert(s.root==ids.d)
map('s');local other=picker.active;await(function() return not other.loading end);map('<Esc>','i');assert(s.root==ids.d)
explorer.close(s);assert(not explorer.state())
print('Neovim explorer integration passed: real filtering, root changes, BFS references, directions, target-window safety and unsaved edits')
rpc.stop();vim.cmd('qa!')
