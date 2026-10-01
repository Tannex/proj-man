-- Save editing state locally even when the Rust host or database is unavailable.
local M = {}
function M.save(record)
  if not record.workspace then return nil, 'Workspace identity is not available yet' end
  if not record.id:match('^[%x%-]+$') then return nil, 'Invalid recovery identity' end
  local root = vim.env.XDG_STATE_HOME or ((vim.env.HOME or '') .. '/.local/state')
  local directory = root .. '/projman/recovery'
  local ok, err = pcall(vim.fn.mkdir, directory, 'p', 448)
  if not ok then return nil, tostring(err) end
  local path = directory .. '/' .. record.id .. '.json'
  local temporary = path .. '.' .. tostring(vim.uv.hrtime()) .. '.tmp'
  local fd, open_err = vim.uv.fs_open(temporary, 'wx', 384)
  if not fd then return nil, open_err end
  local encoded = vim.json.encode(record)
  local written, write_err = vim.uv.fs_write(fd, encoded, 0)
  local synced, sync_err = vim.uv.fs_fsync(fd)
  vim.uv.fs_close(fd)
  if written ~= #encoded or not synced then vim.uv.fs_unlink(temporary); return nil, write_err or sync_err or 'Incomplete recovery write' end
  local renamed, rename_err = vim.uv.fs_rename(temporary, path)
  if not renamed then vim.uv.fs_unlink(temporary); return nil, rename_err end
  return path
end
return M
