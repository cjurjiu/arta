package inputpanel

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/bubbles/v2/textinput"
	"charm.land/lipgloss/v2"
)

// Result messages sent to parent
type SubmitMsg struct{ Value string }
type CancelMsg struct{}

type Mode int

const (
	ModeText Mode = iota // Simple text input (rename, project name)
	ModePath             // Path input with directory listing
)

type dirEntry struct {
	name  string
	isDir bool
}

type Model struct {
	input      textinput.Model
	mode       Mode
	width      int
	height     int
	active     bool
	title      string

	// Directory listing (for ModePath)
	dirEntries []dirEntry
	dirCursor  int
	dirScroll  int
}

func New() Model {
	ti := textinput.New()
	ti.Focus()
	ti.CharLimit = 256

	return Model{
		input: ti,
	}
}

func (m *Model) Activate(mode Mode, title string, initialValue string, width, height int) {
	m.mode = mode
	m.title = title
	m.width = width
	m.height = height
	m.active = true
	m.input.SetValue(initialValue)
	m.input.CursorEnd()
	m.input.Focus()
	m.dirCursor = 0
	m.dirScroll = 0
	if mode == ModePath {
		m.updateDirListing()
	}
}

func (m *Model) Deactivate() {
	m.active = false
	m.input.Blur()
}

func (m Model) Active() bool {
	return m.active
}

func (m Model) Value() string {
	return m.input.Value()
}

func (m Model) Init() tea.Cmd {
	return textinput.Blink
}

func (m Model) Update(msg tea.Msg) (Model, tea.Cmd) {
	if !m.active {
		return m, nil
	}

	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.String() {
		case "esc", "escape":
			m.Deactivate()
			return m, func() tea.Msg { return CancelMsg{} }

		case "enter":
			if m.mode == ModePath && len(m.dirEntries) > 0 {
				// Enter on a directory entry descends into it
				entry := m.dirEntries[m.dirCursor]
				if entry.isDir {
					current := m.expandPath(m.input.Value())
					dir := current
					prefix := ""
					if !strings.HasSuffix(current, "/") {
						dir = filepath.Dir(current)
						prefix = ""
						_ = prefix
					}
					newPath := filepath.Join(dir, entry.name) + "/"
					m.input.SetValue(newPath)
					m.input.CursorEnd()
					m.dirCursor = 0
					m.dirScroll = 0
					m.updateDirListing()
					return m, nil
				}
			}
			val := m.input.Value()
			m.Deactivate()
			return m, func() tea.Msg { return SubmitMsg{Value: val} }

		case "tab":
			if m.mode == ModePath {
				m.tabComplete()
				return m, nil
			}

		case "up":
			if m.mode == ModePath && m.dirCursor > 0 {
				m.dirCursor--
				if m.dirCursor < m.dirScroll {
					m.dirScroll = m.dirCursor
				}
				return m, nil
			}

		case "down":
			if m.mode == ModePath && m.dirCursor < len(m.dirEntries)-1 {
				m.dirCursor++
				maxVisible := m.maxVisibleEntries()
				if m.dirCursor >= m.dirScroll+maxVisible {
					m.dirScroll = m.dirCursor - maxVisible + 1
				}
				return m, nil
			}
		}
	}

	// Pass to textinput for character handling, cursor movement, etc.
	var cmd tea.Cmd
	m.input, cmd = m.input.Update(msg)

	// Update directory listing when text changes
	if m.mode == ModePath {
		m.updateDirListing()
	}

	return m, cmd
}

func (m Model) View() string {
	if !m.active {
		return ""
	}

	dimStyle := lipgloss.NewStyle().Faint(true)
	titleStyle := lipgloss.NewStyle().Bold(true).Foreground(lipgloss.Color("#51afef"))
	selectedStyle := lipgloss.NewStyle().Bold(true).Reverse(true)
	dirStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("#51afef"))

	var b strings.Builder

	// Top border
	b.WriteString(dimStyle.Render(strings.Repeat("─", m.width)) + "\n")

	// Title + input
	b.WriteString(titleStyle.Render(m.title) + "\n")
	b.WriteString(m.input.View() + "\n")

	if m.mode == ModePath {
		b.WriteString(dimStyle.Render(strings.Repeat("─", m.width)) + "\n")

		maxVisible := m.maxVisibleEntries()
		end := m.dirScroll + maxVisible
		if end > len(m.dirEntries) {
			end = len(m.dirEntries)
		}

		for i := m.dirScroll; i < end; i++ {
			entry := m.dirEntries[i]
			name := entry.name
			suffix := ""
			style := dimStyle
			if entry.isDir {
				suffix = "/"
				style = dirStyle
			}

			line := fmt.Sprintf("  %s%s", name, suffix)
			if i == m.dirCursor {
				line = selectedStyle.Render(line)
			} else {
				line = style.Render(line)
			}
			b.WriteString(line + "\n")
		}

		if len(m.dirEntries) == 0 {
			b.WriteString(dimStyle.Render("  (empty)") + "\n")
		}
	}

	// Help line
	help := " esc cancel"
	if m.mode == ModePath {
		help = " esc cancel  tab complete  ↑↓ select  enter open/confirm"
	}
	b.WriteString(dimStyle.Render(help))

	return b.String()
}

// --- Internal helpers ---

func (m *Model) updateDirListing() {
	path := m.expandPath(m.input.Value())

	dir := path
	prefix := ""
	if !strings.HasSuffix(path, "/") {
		dir = filepath.Dir(path)
		prefix = strings.ToLower(filepath.Base(path))
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		m.dirEntries = nil
		return
	}

	m.dirEntries = nil
	for _, e := range entries {
		name := e.Name()
		if strings.HasPrefix(name, ".") {
			continue
		}
		if prefix != "" && !strings.HasPrefix(strings.ToLower(name), prefix) {
			continue
		}
		m.dirEntries = append(m.dirEntries, dirEntry{name: name, isDir: e.IsDir()})
	}

	if m.dirCursor >= len(m.dirEntries) {
		m.dirCursor = len(m.dirEntries) - 1
	}
	if m.dirCursor < 0 {
		m.dirCursor = 0
	}
}

func (m *Model) tabComplete() {
	path := m.expandPath(m.input.Value())

	dir := path
	prefix := ""
	if !strings.HasSuffix(path, "/") {
		dir = filepath.Dir(path)
		prefix = filepath.Base(path)
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}

	var matches []string
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		name := e.Name()
		if strings.HasPrefix(strings.ToLower(name), strings.ToLower(prefix)) {
			matches = append(matches, name)
		}
	}

	if len(matches) == 1 {
		m.input.SetValue(filepath.Join(dir, matches[0]) + "/")
		m.input.CursorEnd()
	} else if len(matches) > 1 {
		common := matches[0]
		for _, match := range matches[1:] {
			for i := range common {
				if i >= len(match) || common[i] != match[i] {
					common = common[:i]
					break
				}
			}
		}
		if len(common) > len(prefix) {
			m.input.SetValue(filepath.Join(dir, common))
			m.input.CursorEnd()
		}
	}

	m.dirCursor = 0
	m.dirScroll = 0
	m.updateDirListing()
}

func (m Model) expandPath(path string) string {
	if strings.HasPrefix(path, "~") {
		home, _ := os.UserHomeDir()
		path = home + path[1:]
	}
	return path
}

func (m Model) maxVisibleEntries() int {
	// Reserve lines for: border, title, input, separator, help
	available := m.height - 5
	if available < 3 {
		available = 3
	}
	return available
}
