-- Guided typed values. Serialization is kept out of the entry interaction.
local M = {}
local rpc = require('projman.rpc')
local function unset(value) return value == nil or value == vim.NIL end
local function clean(text) return tostring(text):gsub('%c', ' ') end
function M.label(field) return field.label and field.label ~= '' and field.label or (field.key:gsub('_', ' '):gsub('^%l', string.upper)) end
function M.display(value)
  if unset(value) then return '(not set)' end
  if type(value) == 'boolean' then return value and 'Yes' or 'No' end
  if type(value) == 'table' then
    local parts = {}; for _, item in ipairs(value) do parts[#parts + 1] = M.display(item) end
    return #parts == 0 and '(empty list)' or table.concat(parts, ', ')
  end
  return clean(value)
end
local function later(fn) vim.schedule(fn) end
local function hint(field)
  local parts = {}
  if field.help and field.help ~= '' then parts[#parts + 1] = field.help end
  if field.type == 'date' then parts[#parts + 1] = 'YYYY-MM-DD' end
  if field.type == 'integer' then parts[#parts + 1] = 'whole number' end
  if field.type == 'number' then parts[#parts + 1] = 'number' end
  if field.min ~= nil and field.min ~= vim.NIL then parts[#parts + 1] = 'minimum ' .. field.min end
  if field.max ~= nil and field.max ~= vim.NIL then parts[#parts + 1] = 'maximum ' .. field.max end
  if field.min_length ~= nil and field.min_length ~= vim.NIL then parts[#parts + 1] = 'minimum length ' .. field.min_length end
  if field.max_length ~= nil and field.max_length ~= vim.NIL then parts[#parts + 1] = 'maximum length ' .. field.max_length end
  return #parts > 0 and (' · ' .. clean(table.concat(parts, '; '))) or ''
end
local function missing(field, value)
  return field.required and (unset(value) or type(value) == 'string' and vim.trim(value) == '' or type(value) == 'table' and #value == 0)
end
local ask
local function scalar(field, value, title, callback, error_message)
  local label = M.label(field)
  local prompt = title .. ' · ' .. label .. (field.required and ' (required; may leave unset for a draft)' or ' (optional)') .. hint(field)
  if error_message then prompt = prompt .. '\n' .. clean(error_message) end
  if field.type == 'enum' or field.type == 'boolean' then
    local choices = {}
    if field.type == 'enum' then
      for _, choice in ipairs(field.choices or {}) do choices[#choices + 1] = { text = choice, value = choice } end
    else choices = { { text = 'Yes', value = true }, { text = 'No', value = false } } end
    choices[#choices + 1] = { text = field.required and 'Leave unset (incomplete draft)' or 'Leave unset', value = vim.NIL }
    -- Present the current/default choice first, including false.
    for index, choice in ipairs(choices) do
      if not unset(value) and choice.value == value then table.remove(choices, index); table.insert(choices, 1, choice); break end
    end
    vim.ui.select(choices, { prompt = prompt, format_item = function(choice) return choice.text .. (choice.value == value and ' (current)' or '') end }, function(choice)
      if not choice then callback(nil, true) else callback(choice.value, false) end
    end)
    return
  end
  local default = unset(value) and '' or tostring(value)
  vim.ui.input({ prompt = prompt .. ': ', default = default }, function(input)
    if input == nil then callback(nil, true); return end
    if vim.trim(input) == '' then callback(vim.NIL, false); return end
    if field.type == 'integer' or field.type == 'number' then
      local ok, number = pcall(vim.json.decode, vim.trim(input))
      if not ok or type(number) ~= 'number' or number ~= number or math.abs(number) == math.huge or field.type == 'integer' and (number % 1 ~= 0 or math.abs(number) > 9007199254740991) then
        later(function() scalar(field, input, title, callback, field.type == 'integer' and 'Enter a whole number, such as 3.' or 'Enter a number, such as 2.5.') end)
        return
      end
      callback(number, false)
    else callback(input, false) end
  end)
end
local function list(field, value, title, callback)
  local items = unset(value) and {} or vim.deepcopy(value)
  if type(items) ~= 'table' then items = {} end
  local function menu()
    local choices = { { action = 'done', text = 'Done (' .. #items .. ' items)' }, { action = 'add', text = 'Add item' } }
    for index, item in ipairs(items) do choices[#choices + 1] = { action = 'edit', index = index, text = index .. '. ' .. M.display(item) } end
    choices[#choices + 1] = { action = 'unset', text = field.required and 'Leave unset (incomplete draft)' or 'Leave unset' }
    vim.ui.select(choices, { prompt = title .. ' · ' .. M.label(field) .. hint(field), format_item = function(item) return item.text end }, function(choice)
      if not choice then callback(nil, true); return end
      if choice.action == 'done' then callback(items, false); return end
      if choice.action == 'unset' then callback(vim.NIL, false); return end
      local item_field = { key = field.key, label = 'Item', type = field.items, choices = field.choices, required = true }
      local function edit(index)
        local current
        if index then current = items[index] end
        scalar(item_field, current, title .. ' · ' .. M.label(field), function(result, cancelled)
          if cancelled then later(menu); return end
          if not unset(result) then if index then items[index] = result else items[#items + 1] = result end end
          later(menu)
        end)
      end
      if choice.action == 'add' then edit(); return end
      vim.ui.select({ 'Edit item', 'Remove item', 'Back' }, { prompt = M.display(items[choice.index]) }, function(action)
        if action == 'Edit item' then edit(choice.index)
        else if action == 'Remove item' then table.remove(items, choice.index) end; later(menu) end
      end)
    end)
  end
  menu()
end
ask = function(context, field, value, title, callback)
  local function receive(candidate, cancelled)
    if cancelled then callback(nil, true); return end
    rpc.request('property.validate', { type_key = context.type_key, schema_revision = context.schema_revision, key = field.key, value = candidate }, function(err)
      if not err then callback(candidate, false); return end
      if err.code == 'usage' then
        vim.notify('Guided fields need the updated Rust host. Run :Lazy build projman and restart Neovim.', vim.log.levels.ERROR, { title = 'ProjMan' })
        callback(nil, true); return
      end
      if err.code == 'validation' then
        vim.notify(M.label(field) .. ': ' .. err.message:gsub('^' .. field.key .. ':%s*', ''), vim.log.levels.WARN, { title = 'ProjMan' })
        later(function() ask(context, field, candidate, title, callback) end)
      else
        vim.ui.select({ 'Retry validation', 'Cancel' }, { prompt = err.message }, function(choice)
          if choice == 'Retry validation' then later(function() receive(candidate, false) end) else callback(nil, true) end
        end)
      end
    end)
  end
  if field.type == 'list' then list(field, value, title, receive) else scalar(field, value, title, receive) end
end
M.ask = ask
function M.choose_field(definition, values, title, save_label, callback, close_label)
  local choices = {}
  for _, field in ipairs(definition.properties) do
    choices[#choices + 1] = { field = field, text = (missing(field, values[field.key]) and '! ' or '') .. M.label(field) .. ' — ' .. M.display(values[field.key]) .. (field.required and ' *' or '') }
  end
  choices[#choices + 1] = { save = true, text = save_label or 'Save and return to notes' }
  choices[#choices + 1] = { close = true, text = close_label or 'Return to notes without saving' }
  vim.ui.select(choices, { prompt = title .. ' · choose a property (* required)', format_item = function(choice) return choice.text end }, function(choice) callback(choice) end)
end
function M.create(seed, opts, callback)
  opts = opts or {}
  local values = vim.deepcopy(seed.properties)
  local definition = seed.type
  local context = { type_key = seed.type_key, schema_revision = seed.schema_revision }
  local function finish()
    local missing_labels = {}
    for _, field in ipairs(definition.properties) do if missing(field, values[field.key]) then missing_labels[#missing_labels + 1] = M.label(field) end end
    callback(values, false, missing_labels)
  end
  local function review()
    local draft = false
    for _, field in ipairs(definition.properties) do draft = draft or missing(field, values[field.key]) end
    M.choose_field(definition, values, 'New ' .. definition.name, draft and 'Create incomplete draft and open notes' or 'Create node and open notes', function(choice)
      if not choice or choice.close then callback(nil, true); return end
      if choice.save then finish(); return end
      ask(context, choice.field, values[choice.field.key], definition.name, function(value, cancelled)
        if not cancelled then values[choice.field.key] = value end
        later(review)
      end)
    end, 'Cancel creation')
  end
  if opts.form then review(); return end
  local required = {}
  for _, field in ipairs(definition.properties) do
    if field.key == definition.display_property or missing(field, values[field.key]) then required[#required + 1] = field end
  end
  local function step(index)
    local field = required[index]
    if not field then finish(); return end
    ask(context, field, values[field.key], 'New ' .. definition.name .. ' · ' .. index .. '/' .. #required, function(value, cancelled)
      if cancelled then callback(nil, true); return end
      values[field.key] = value
      later(function() step(index + 1) end)
    end)
  end
  step(1)
end
return M
