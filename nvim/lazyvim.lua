-- Drop-in local LazyVim spec. Load with dofile("/path/to/proj-man/nvim/lazyvim.lua").
local source = debug.getinfo(1, 'S').source:sub(2)
local directory = vim.fn.fnamemodify(vim.uv.fs_realpath(source) or source, ':h')
local root = vim.fn.fnamemodify(directory, ':h')
return {
  {
    name = 'projman', dir = directory, main = 'projman', lazy = true,
    cmd = {
      'ProjManExplore', 'ProjManExplorerRoot', 'ProjManExplorerRefresh', 'ProjManFind', 'ProjManHealth',
      'ProjManNew', 'ProjManOpen', 'ProjManSearch', 'ProjManTypes', 'ProjManTypeNew', 'ProjManRelationTypeNew',
      'ProjManLinks', 'ProjManLink', 'ProjManOutline', 'ProjManBacklinks', 'ProjManNextField', 'ProjManPreviousField',
      'ProjManRecover', 'ProjManConflict', 'ProjManBack', 'ProjManRetry', 'ProjManReparent', 'ProjManReorder',
      'ProjManReconcile', 'ProjManProposals', 'ProjManReview', 'ProjManUnstage',
      'ProjManProperties',
    },
    opts = {},
    build = function()
      local result = vim.system({ 'cargo', 'build', '--manifest-path', root .. '/Cargo.toml', '-p', 'projman-cli', '--release', '--locked' }, { text = true }):wait()
      if result.code ~= 0 then error(result.stderr or 'ProjMan build failed') end
    end,
    keys = {
      { '<leader>Pe', function() require('projman').explorer_toggle() end, desc = 'ProjMan explorer' },
      { '<leader>Pr', '<cmd>ProjManExplorerRoot<cr>', desc = 'Choose graph root' },
      { '<leader>Pf', '<cmd>ProjManFind<cr>', desc = 'Find node' },
      { '<leader>Pn', '<cmd>ProjManNew<cr>', desc = 'New node' },
      { '<leader>Pv', '<cmd>ProjManProperties<cr>', desc = 'Edit node properties' },
      { '<leader>Pl', '<cmd>ProjManLink<cr>', desc = 'Link node' },
      { '<leader>PL', '<cmd>ProjManLinks<cr>', desc = 'Node relationships' },
      { '<leader>Pb', '<cmd>ProjManBacklinks<cr>', desc = 'Node backlinks' },
      { '<leader>Pt', '<cmd>ProjManTypes<cr>', desc = 'Edit types' },
      { '<leader>Po', '<cmd>ProjManOutline<cr>', desc = 'Planning outline' },
      { '<leader>Pp', '<cmd>ProjManProposals<cr>', desc = 'Review proposals' },
      { '<leader>Pc', '<cmd>ProjManConflict<cr>', desc = 'Compare save conflict' },
      { '<leader>PR', '<cmd>ProjManRecover<cr>', desc = 'Recover edits' },
      { '<leader>P]', '<cmd>ProjManNextField<cr>', desc = 'Next property' },
      { '<leader>P[', '<cmd>ProjManPreviousField<cr>', desc = 'Previous property' },
      { '<leader>P?', '<cmd>ProjManHealth<cr>', desc = 'ProjMan health' },
    },
  },
  { 'folke/which-key.nvim', optional = true, opts = { spec = { { '<leader>P', group = 'ProjMan', icon = '󰙅' } } } },
  { 'folke/edgy.nvim', optional = true, opts = function(_, opts)
    opts.left = opts.left or {}
    table.insert(opts.left, { title = 'ProjMan', ft = 'projman-explorer', size = { width = 44 } })
  end },
}
