local M = { pending = {}, sequence = 0, workspace = nil }
local options = { command = { 'projman' }, timeout = 10000 }
local job, incoming
local function finish(id, err, result)
  local entry = M.pending[id]
  if not entry then return end
  M.pending[id] = nil
  entry.timer:stop()
  entry.timer:close()
  vim.schedule(function() entry.callback(err, result) end)
end
local function disconnected(message, exited_job)
  if exited_job and exited_job ~= job then return end
  local previous = job
  job = nil
  if previous and not exited_job then vim.fn.jobstop(previous) end
  local ids = vim.tbl_keys(M.pending)
  for _, id in ipairs(ids) do finish(id, { code = 'unavailable', message = message }) end
end
function M.setup(opts)
  options = vim.tbl_extend('force', options, opts or {})
end
local function start()
  if job and job > 0 then return true end
  incoming = ''
  local command = vim.deepcopy(options.command)
  vim.list_extend(command, { 'serve', '--stdio' })
  job = vim.fn.jobstart(command, {
    env = options.env,
    on_stdout = function(_, chunks)
      incoming = incoming .. table.concat(chunks, '\n')
      while true do
        local ending = incoming:find('\n', 1, true)
        if not ending then break end
        local line = incoming:sub(1, ending - 1)
        incoming = incoming:sub(ending + 1)
        local ok, response = pcall(vim.json.decode, line)
        if ok and type(response) == 'table' and response.protocol_version == 1 then
          finish(response.id, response.ok and nil or response.error, response.data)
        else
          disconnected('Invalid response from ProjMan host')
          return
        end
      end
      if #incoming > 4 * 1024 * 1024 then disconnected('ProjMan response exceeds 4 MiB') end
    end,
    on_stderr = function(_, chunks)
      local text = table.concat(chunks, '\n')
      if text:match('%S') then vim.schedule(function() vim.notify(text, vim.log.levels.WARN) end) end
    end,
    on_exit = function(id) disconnected('ProjMan host exited; local edits are preserved', id) end,
  })
  if job <= 0 then job = nil; return false end
  return true
end
function M.request(method, params, callback)
  callback = callback or function(err) if err then vim.notify(err.message, vim.log.levels.ERROR) end end
  if not start() then
    vim.schedule(function() callback({ code = 'unavailable', message = 'Cannot start projman; configure setup({command={...}})' }) end)
    return
  end
  M.sequence = M.sequence + 1
  local id = M.sequence
  local timer = vim.uv.new_timer()
  M.pending[id] = { callback = callback, timer = timer }
  timer:start(options.timeout, 0, function()
    local message = 'ProjMan ' .. method .. ' timed out'
    if method == 'change.apply' or method == 'recovery.restore' then message = message .. '; the write may have committed. Check its operation receipt before retrying.' end
    finish(id, { code = 'unavailable', message = message })
  end)
  local message = vim.json.encode({ protocol_version = 1, id = id, method = method, params = params or {} })
  local ok = vim.fn.chansend(job, message .. '\n')
  if ok == 0 then finish(id, { code = 'unavailable', message = 'Failed to send request to ProjMan' }) end
  return id
end
function M.initialize(callback)
  M.request('initialize', {}, function(err, result)
    if not err then M.workspace = result.workspace end
    if callback then callback(err, result) end
  end)
end
function M.stop()
  if job then vim.fn.jobstop(job) end
end
return M
