local root = assert(vim.env.PROJMAN_TEST_ROOT)
vim.opt.runtimepath:prepend(root .. '/nvim')
local entry, rpc = require('projman.entry'), require('projman.rpc')
local prompts, menus, validations, steps = {}, {}, {}, {}
local old_input, old_select = vim.ui.input, vim.ui.select
vim.notify = function() end
local function answer(kind)
  local step = table.remove(steps, 1)
  assert(step and step.kind == kind, 'Unexpected UI ' .. kind)
  return step
end
vim.ui.input = function(opts, cb)
  prompts[#prompts + 1] = opts
  local step = answer('input')
  if step.check then step.check(opts) end
  vim.schedule(function() cb(step.value) end)
end
vim.ui.select = function(items, opts, cb)
  menus[#menus + 1] = { items = items, opts = opts }
  local step = answer('select')
  local selected
  if step.pick then for _, item in ipairs(items) do if step.pick(item) then selected = item; break end end; assert(selected, 'Menu choice not found: ' .. opts.prompt) end
  if step.check then step.check(items, opts) end
  vim.schedule(function() cb(selected) end)
end
rpc.request = function(method, params, cb)
  assert(method == 'property.validate')
  validations[#validations + 1] = params
  local err
  if params.key == 'hours' and params.value ~= vim.NIL and params.value > 8 then err = { code = 'validation', message = 'hours: value is outside the allowed range' } end
  vim.schedule(function() cb(err, { value = params.value }) end)
end
local function await(fn)
  local done, value, cancelled = false, nil, nil
  fn(function(v, c) done, value, cancelled = true, v, c end)
  assert(vim.wait(3000, function() return done end, 5), 'Entry did not finish')
  assert(#steps == 0, 'Unused UI answers')
  return value, cancelled
end
local context = { type_key = 'task', schema_revision = 1 }
steps = { { kind = 'input', value = 'My "quoted" title' } }
local value = await(function(cb) entry.ask(context, { key = 'title', type = 'string' }, nil, 'Task', cb) end)
assert(value == 'My "quoted" title', 'Text inputs never require or consume JSON quotes')
steps = { { kind = 'select', pick = function(item) return item.value == false end, check = function(items) assert(items[1].value == false, 'Keep a current false value first') end } }
value = await(function(cb) entry.ask(context, { key = 'ready', type = 'boolean' }, false, 'Task', cb) end)
assert(value == false and validations[#validations].value == false)
steps = { { kind = 'input', value = 'many' }, { kind = 'input', value = '10' }, { kind = 'input', value = '4' } }
value = await(function(cb) entry.ask(context, { key = 'hours', label = 'Hours', type = 'integer', max = 8 }, nil, 'Task', cb) end)
assert(value == 4, 'Bad input and failed constraints must reprompt the same field')
steps = {
  { kind = 'select', pick = function(item) return item.action == 'add' end },
  { kind = 'input', value = 'tag, with comma' },
  { kind = 'select', pick = function(item) return item.action == 'add' end },
  { kind = 'input', value = 'second' },
  { kind = 'select', pick = function(item) return item.action == 'edit' and item.index == 2 end },
  { kind = 'select', pick = function(item) return item == 'Remove item' end },
  { kind = 'select', pick = function(item) return item.action == 'done' end },
}
value = await(function(cb) entry.ask(context, { key = 'tags', type = 'list', items = 'string' }, {}, 'Task', cb) end)
assert(#value == 1 and value[1] == 'tag, with comma', 'Lists use item controls, not JSON or comma splitting')
local seed = { type_key = 'task', schema_revision = 1, properties = { status = 'todo', ready = false },
  type = { key = 'task', name = 'Task', display_property = 'title', properties = {
    { key = 'title', label = 'Title', type = 'string', required = true },
    { key = 'status', type = 'enum', choices = { 'todo', 'done' }, required = true },
    { key = 'ready', type = 'boolean', required = true },
    { key = 'hours', type = 'integer' },
  } } }
steps = { { kind = 'input', value = 'Only missing facts' } }
value = await(function(cb) entry.create(seed, {}, cb) end)
assert(value.title == 'Only missing facts' and value.status == 'todo' and value.ready == false)
steps = { { kind = 'input' } }
local _, cancelled = await(function(cb) entry.create(seed, {}, cb) end)
assert(cancelled, 'Escape cancels creation')
steps = { { kind = 'input', value = '' } }
value = await(function(cb) entry.create(seed, {}, cb) end)
assert(value.title == vim.NIL, 'Required facts may be left unset for a draft')
steps = {
  { kind = 'select', pick = function(item) return item.field and item.field.key == 'title' end },
  { kind = 'input', value = 'Form title' },
  { kind = 'select', pick = function(item) return item.save end },
}
value = await(function(cb) entry.create(seed, { form = true }, cb) end)
assert(value.title == 'Form title')
vim.ui.input, vim.ui.select = old_input, old_select
print('Guided entry checks passed: text, boolean false, retries, list controls, defaults, cancellation, incomplete drafts and form mode')
vim.cmd('qa!')
