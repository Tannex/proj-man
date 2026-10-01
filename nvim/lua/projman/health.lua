local M = {}
function M.check()
  vim.health.start('ProjMan')
  local plugin = require('projman')
  local config = plugin.config or { command = require('projman.runtime').command() }
  local executable = config.command[1]
  if vim.fn.executable(executable) ~= 1 then
    vim.health.error('projman executable is missing', { 'Run :Lazy build projman, or cargo build --workspace --locked in the checkout.' })
    return
  end
  vim.health.ok('Executable: ' .. executable)
  local command = vim.deepcopy(config.command)
  vim.list_extend(command, { '--json', 'doctor' })
  local result = vim.system(command, { text = true, env = config.env }):wait(12000)
  local parsed, envelope = pcall(vim.json.decode, result.stdout or '')
  if result.code == 0 and parsed and envelope.ok then
    vim.health.ok('Neo4j connected; workspace: ' .. envelope.data.workspace)
  else
    vim.health.warn(parsed and envelope.error and envelope.error.message or 'Could not connect to Neo4j', {
      'Start Neo4j with the README instructions, then run projman workspace init.',
      'Neovim inherits PROJMAN_NEO4J_URI, PROJMAN_NEO4J_PASSWORD and PROJMAN_WORKSPACE; setup({env={...}}) can also supply these.',
    })
  end
  vim.health.info('Use <leader>Pe to open the explorer with the LazyVim spec, or :ProjManExplore.')
end
return M
