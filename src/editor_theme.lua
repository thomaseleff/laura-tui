-- Laura's theme (the startup wizard's `y`): a `laura` colorscheme in the file view's Nord palette,
-- foreground only, and a gutter that doesn't shift on a Ctrl+L flip.
-- VimEnter runs it after the user's config and plugins, and only inside Laura.
vim.api.nvim_create_autocmd('VimEnter', {
  once = true,
  callback = function()
    vim.cmd('highlight clear')
    vim.g.colors_name = 'laura'
    -- C (Vim group -> color) comes from assets/nord.tmTheme; see GROUPS in src/editor.rs.
    -- Treesitter's @ groups link to the classic groups by default, so these cover them too.
    for name, fg in pairs(C) do
      vim.api.nvim_set_hl(0, name, { fg = fg })
    end
    local dim = C.Comment
    local groups = {
      Normal = { fg = C.Normal, bg = 'NONE' },
      -- Vim's Rust syntax names traits and enums apart from types; the file view colors them as types.
      rustTrait = { link = 'Type' },
      rustEnum = { link = 'Type' },
      -- The file view's cursor band spans the gutter, its number stays dim (src/view.rs BAND).
      CursorLine = { bg = '#434C5E' },
      LineNr = { fg = dim },
      CursorLineNr = { fg = dim, bg = '#434C5E' },
      CursorLineSign = { bg = '#434C5E' },
      SignColumn = { bg = 'NONE' },
    }
    for name, hl in pairs(groups) do
      vim.api.nvim_set_hl(0, name, hl)
    end
    vim.opt.number = true
    -- At least 2 digits and a space, so the text starts at gw + 1, as in the file view
    -- (Panel::gutter_width, panel::DECOR). A sign (a thread's ✎, a plugin's sign) shows in place of
    -- its line's number, and its 2 cells fit on a short file.
    vim.opt.numberwidth = 3
    vim.opt.cursorline = true
    vim.opt.signcolumn = 'number'
  end,
})
