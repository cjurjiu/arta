package workspace

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

type Project struct {
	Name string `json:"name"`
	Path string `json:"path"`
}

type Session struct {
	ID        string `json:"id"`
	Project   string `json:"project"`
	Created   string `json:"created"`
	WindowIdx int    `json:"-"` // TUIOS window index, not persisted
	Alive     bool   `json:"-"`
}

type Workspace struct {
	Projects []Project `json:"projects"`
	Sessions []Session `json:"sessions"`
	filePath string
}

func dataDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "arta", "data")
}

func New() *Workspace {
	dir := dataDir()
	return &Workspace{
		filePath: filepath.Join(dir, "workspace.json"),
	}
}

func (w *Workspace) Load() error {
	data, err := os.ReadFile(w.filePath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	return json.Unmarshal(data, w)
}

func (w *Workspace) Save() error {
	dir := filepath.Dir(w.filePath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(w, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(w.filePath, data, 0644)
}

func (w *Workspace) AddProject(name, path string) {
	for _, p := range w.Projects {
		if p.Name == name {
			return
		}
	}
	w.Projects = append(w.Projects, Project{Name: name, Path: path})
	w.Save()
}

func (w *Workspace) RemoveProject(name string) {
	// Remove sessions for this project
	filtered := w.Sessions[:0]
	for _, s := range w.Sessions {
		if s.Project != name {
			filtered = append(filtered, s)
		}
	}
	w.Sessions = filtered

	// Remove project
	projects := w.Projects[:0]
	for _, p := range w.Projects {
		if p.Name != name {
			projects = append(projects, p)
		}
	}
	w.Projects = projects
	w.Save()
}

func (w *Workspace) RenameProject(oldName, newName string) {
	for i, p := range w.Projects {
		if p.Name == oldName {
			w.Projects[i].Name = newName
		}
	}
	for i, s := range w.Sessions {
		if s.Project == oldName {
			w.Sessions[i].Project = newName
		}
	}
	w.Save()
}

func (w *Workspace) SwapProjects(i, j int) {
	if i >= 0 && j >= 0 && i < len(w.Projects) && j < len(w.Projects) {
		w.Projects[i], w.Projects[j] = w.Projects[j], w.Projects[i]
		w.Save()
	}
}

func (w *Workspace) CreateSession(projectName string) *Session {
	count := 0
	for _, s := range w.Sessions {
		if s.Project == projectName {
			count++
		}
	}
	session := Session{
		ID:      fmt.Sprintf("%s-%d", projectName, count+1),
		Project: projectName,
		Created: time.Now().UTC().Format(time.RFC3339),
		Alive:   true,
	}
	w.Sessions = append(w.Sessions, session)
	w.Save()
	return &w.Sessions[len(w.Sessions)-1]
}

func (w *Workspace) RemoveSession(id string) {
	filtered := w.Sessions[:0]
	for _, s := range w.Sessions {
		if s.ID != id {
			filtered = append(filtered, s)
		}
	}
	w.Sessions = filtered
	w.Save()
}

func (w *Workspace) GetProjectPath(name string) string {
	for _, p := range w.Projects {
		if p.Name == name {
			return p.Path
		}
	}
	return ""
}

func (w *Workspace) SessionsForProject(name string) []Session {
	var result []Session
	for _, s := range w.Sessions {
		if s.Project == name {
			result = append(result, s)
		}
	}
	return result
}

func (w *Workspace) FindSession(id string) *Session {
	for i, s := range w.Sessions {
		if s.ID == id {
			return &w.Sessions[i]
		}
	}
	return nil
}

func (w *Workspace) RenameSession(oldID, newID string) {
	for i, s := range w.Sessions {
		if s.ID == oldID {
			w.Sessions[i].ID = newID
			break
		}
	}
	w.Save()
}

func (w *Workspace) SwapSessionInProject(id string, direction int) {
	// Find the session and its project
	var project string
	for _, s := range w.Sessions {
		if s.ID == id {
			project = s.Project
			break
		}
	}
	if project == "" {
		return
	}

	// Collect indices of sessions in this project
	var indices []int
	for i, s := range w.Sessions {
		if s.Project == project {
			indices = append(indices, i)
		}
	}

	// Find position within project sessions
	for pos, idx := range indices {
		if w.Sessions[idx].ID == id {
			targetPos := pos + direction
			if targetPos >= 0 && targetPos < len(indices) {
				targetIdx := indices[targetPos]
				w.Sessions[idx], w.Sessions[targetIdx] = w.Sessions[targetIdx], w.Sessions[idx]
				w.Save()
			}
			return
		}
	}
}
