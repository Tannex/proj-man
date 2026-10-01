local M = {}
local source = debug.getinfo(1, 'S').source:sub(2)
M.root = vim.fn.fnamemodify(vim.uv.fs_realpath(source) or source, ':h:h:h:h')
function M.command()
  local candidates = {}
  for _, profile in ipairs({ 'release', 'debug' }) do
    local path = M.root .. '/target/' .. profile .. '/projman'
    local stat = vim.uv.fs_stat(path)
    if stat and vim.fn.executable(path) == 1 then candidates[#candidates + 1] = { path = path, time = stat.mtime.sec, nanos = stat.mtime.nsec } end
  end
  table.sort(candidates, function(a, b) return a.time == b.time and a.nanos > b.nanos or a.time > b.time end)
  if #candidates > 0 then return { candidates[1].path } end
  local installed = vim.fn.exepath('projman')
  return { installed ~= '' and installed or 'projman' }
end
return M
