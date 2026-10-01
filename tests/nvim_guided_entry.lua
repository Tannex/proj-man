local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
vim.cmd('runtime plugin/projman.lua')
vim.g.mapleader = ' '
local plugin, rpc = require('projman'), require('projman.rpc')
plugin.setup({ command = { root .. '/target/debug/projman' } })
local steps, messages = {}, {}
vim.notify = function(message) messages[#messages + 1] = message end
local function take(kind)
  local step = table.remove(steps, 1)
  assert(step and step.kind == kind, 'Unexpected ' .. kind .. ' interaction')
  return step
end
vim.ui.input = function(opts, cb)
  local step = take('input'); if step.check then step.check(opts) end
  vim.schedule(function() cb(step.value) end)
end
vim.ui.select = function(items, opts, cb)
  local step = take('select'); local selected
  if step.pick then for _, item in ipairs(items) do if step.pick(item) then selected = item; break end end; assert(selected, 'Missing choice: ' .. opts.prompt) end
  if step.check then step.check(items, opts) end
  vim.schedule(function() cb(selected) end)
end
local function await(predicate) assert(vim.wait(10000, predicate, 10), 'Timed out') end
local function request(method, params)
  local done, err, result = false, nil, nil
  rpc.request(method, params, function(e, r) done, err, result = true, e, r end)
  await(function() return done end); assert(not err, vim.inspect(err)); return result
end
steps = {
  { kind = 'select', pick = function(ty) return ty.key == 'task' end },
  { kind = 'input', value = 'x', check = function(opts) assert(opts.prompt:find('Title',1,true)) end },
  { kind = 'input', value = 'Guided title' },
  { kind = 'input', value = 'not a date', check = function(opts) assert(opts.prompt:find('YYYY-MM-DD',1,true)) end },
  { kind = 'input', value = '2026-10-02' },
}
vim.cmd('ProjManNew')
await(function()
  local s = plugin.buffers[vim.api.nvim_get_current_buf()]
  return #steps == 0 and s and s.node.revision == 1 and not s.saving
end)
vim.cmd('stopinsert')
local buf, state = vim.api.nvim_get_current_buf(), plugin.buffers[vim.api.nvim_get_current_buf()]
assert(request('node.get',{id=state.node.id}).properties.title == 'Guided title')
assert(state.node.properties.status == 'todo' and state.node.properties.ready == false)
assert(vim.api.nvim_win_get_cursor(0)[1] == #state.definition.properties + 3, 'Focus moves to the Markdown body')
vim.api.nvim_buf_set_lines(buf,-2,-1,false,{'Notes typed before opening properties.'})
local function field(key) return function(item) return item.field and item.field.key == key end end
steps = {
  { kind = 'select', pick = field('status') },
  { kind = 'select', pick = function(item) return item.value == 'done' end },
  { kind = 'select', pick = field('ready') },
  { kind = 'select', pick = function(item) return item.value == false end, check = function(items) assert(items[1].value == false) end },
  { kind = 'select', pick = field('hours') },
  { kind = 'input', value = '99' },
  { kind = 'input', value = '4' },
  { kind = 'select', pick = field('tags') },
  { kind = 'select', pick = function(item) return item.action == 'add' end },
  { kind = 'input', value = 'API' },
  { kind = 'select', pick = function(item) return item.action == 'add' end },
  { kind = 'input', value = 'UI' },
  { kind = 'select', pick = function(item) return item.action == 'done' end },
  { kind = 'select', pick = function(item) return item.save end },
}
vim.cmd('ProjManProperties')
await(function() return #steps == 0 and not state.entry_busy and not state.saving and not vim.bo[buf].modified end)
local saved = request('node.get',{id=state.node.id})
assert(saved.properties.status == 'done' and saved.properties.ready == false)
assert(saved.properties.hours == 4 and saved.properties.tags[2] == 'UI')
assert(saved.body == 'Notes typed before opening properties.')
-- Cancel leaves the current node and database untouched.
local before = request('workspace.show',{})
steps = { { kind = 'input', value = 'Cancelled title' }, { kind = 'input' } }
vim.cmd('ProjManNew task')
await(function() return #steps == 0 and not plugin.entry_busy end)
assert(request('workspace.show',{}).graph_revision == before.graph_revision)
assert(vim.api.nvim_get_current_buf() == buf)
-- Unknown required facts can still be saved deliberately as an incomplete draft.
steps = { { kind = 'input', value = '' }, { kind = 'input', value = '' } }
vim.cmd('ProjManNew task')
await(function()
  local s = plugin.buffers[vim.api.nvim_get_current_buf()]
  return #steps == 0 and not plugin.entry_busy and s and s.node.id ~= state.node.id and s.node.revision == 1 and not s.saving
end)
assert(#plugin.buffers[vim.api.nvim_get_current_buf()].node.missing == 2)
vim.cmd('stopinsert')
-- Bang keeps the advanced raw-buffer workflow available and never auto-saves.
before = request('workspace.show',{})
vim.cmd('ProjManNew! note')
await(function() local s = plugin.buffers[vim.api.nvim_get_current_buf()]; return s and s.node.type_key == 'note' end)
assert(plugin.buffers[vim.api.nvim_get_current_buf()].node.revision == 0)
assert(request('workspace.show',{}).graph_revision == before.graph_revision)
print('Neovim guided commands passed: required steps, defaults, typed edits, save action, cancel, notes and incomplete drafts')
rpc.stop();vim.cmd('qa!')
