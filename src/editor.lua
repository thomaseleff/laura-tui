-- Laura's threads as read-only diagnostics. Laura calls laura_threads(json) over --listen on every
-- change; see src/editor.rs.
local ns = vim.api.nvim_create_namespace('laura')
-- One extmark per thread key, so a thread follows edits: vim.diagnostic.get() keeps reporting
-- the line a diagnostic was set on.
local track = vim.api.nvim_create_namespace('laura_track')
local INFO = vim.diagnostic.severity.INFO
vim.diagnostic.config({
  virtual_lines = false,
  virtual_text = { prefix = '✎', format = function(d) return d.user_data.newest end },
  signs = { text = { [INFO] = '✎' } },
}, ns)
-- Re-applied after a reload (`:e!`, a changed file): the reload leaves the tracking marks where
-- the discarded edits put them.
local last
function _G.laura_threads(json)
  last = json
  local p = vim.json.decode(json)
  local buf = vim.fn.bufadd(p.path)
  local diags = {}
  for _, t in ipairs(p.threads) do
    local lines = {}
    for _, n in ipairs(t.notes) do
      table.insert(lines, n.author .. ': ' .. n.body)
    end
    local at = vim.api.nvim_buf_get_extmark_by_id(buf, track, t.key + 1, {})
    local lnum = at[1] or t.start - 1
    table.insert(diags, {
      lnum = lnum,
      end_lnum = lnum + t['end'] - t.start,
      col = 0,
      severity = INFO,
      source = 'laura',
      message = table.concat(lines, '\n'),
      user_data = { key = t.key, newest = lines[#lines] },
    })
  end
  vim.api.nvim_buf_clear_namespace(buf, track, 0, -1)
  for _, d in ipairs(diags) do
    vim.api.nvim_buf_set_extmark(buf, track, d.lnum, 0, { id = d.user_data.key + 1, strict = false })
  end
  -- An open diagnostic float would keep showing the old threads until the cursor moves. Its
  -- window var named for its scope (`focus_id`) tells it from a language server's hover, which stays.
  local w = vim.b[buf].lsp_floating_preview
  if w and vim.api.nvim_win_is_valid(w) and (vim.w[w].line or vim.w[w].cursor or vim.w[w].buffer) then
    vim.api.nvim_win_close(w, true)
  end
  vim.diagnostic.set(ns, buf, diags)
  return #diags
end
vim.api.nvim_create_autocmd('BufReadPost', {
  callback = function(ev)
    if last then
      vim.api.nvim_buf_clear_namespace(ev.buf, track, 0, -1)
      laura_threads(last)
    end
  end,
})
-- Laura refuses to close the pane while this file exists; see Tab::edits_guard. BufModifiedSet
-- misses `:w`, `:e!` and `:set nomodified`, hence the rest; a buffer being deleted doesn't count.
local dirty = vim.env.LAURA_DIRTY
local function sync(ev)
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    local gone = ev.event == 'BufDelete' and b == ev.buf
    if not gone and vim.bo[b].modified and vim.bo[b].buftype == '' then
      local f = io.open(dirty, 'w')
      if f then f:close() end
      return
    end
  end
  os.remove(dirty)
end
vim.api.nvim_create_autocmd({ 'BufModifiedSet', 'BufWritePost', 'BufReadPost', 'BufDelete' }, {
  callback = sync,
})
-- Its own autocmd: for Buf* events a pattern matches file names.
vim.api.nvim_create_autocmd('OptionSet', { pattern = 'modified', callback = sync })
