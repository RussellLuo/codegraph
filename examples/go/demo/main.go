package main

import (
	"fmt"
	"time"
)

type User struct {
	ID        int
	Username  string
	Email     string
	Address   *Address
	CreatedAt time.Time
}

func NewUser(id int, username, email string) *User {
	return &User{
		ID:        id,
		Username:  username,
		Email:     email,
		CreatedAt: time.Now(),
	}
}

func (u *User) DisplayInfo() {
	fmt.Printf("User ID: %d\n", u.ID)
	fmt.Printf("Username: %s\n", u.Username)
	fmt.Printf("Email: %s\n", u.Email)
	fmt.Printf("Created At: %s\n", u.CreatedAt.Format("2006-01-02 15:04:05"))
}

func (u *User) UpdateEmail(newEmail string) {
	u.Email = newEmail
	fmt.Printf("Email updated to: %s\n", newEmail)
}

func (u *User) SetAddress(addr *Address, hobby *Hobby) {
	u.Address = addr
}

func main() {
	user := NewUser(1, "gopher", "gopher@example.com")

	fmt.Println("User information:")
	user.DisplayInfo()

	fmt.Println("\nUpdating email...")
	user.UpdateEmail("newemail@example.com")

	fmt.Println("\nUpdated user information:")
	user.DisplayInfo()
}
