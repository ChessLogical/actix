#!/bin/bash

# Variables
DB_NAME="my_database"
DB_USER="my_user"
DB_PASSWORD="my_password"
ROOT_USER="root"

# Connect to MySQL and execute commands
sudo mysql -u $ROOT_USER <<MYSQL_SCRIPT
DROP DATABASE IF EXISTS $DB_NAME;
CREATE DATABASE $DB_NAME;

CREATE USER IF NOT EXISTS '$DB_USER'@'localhost' IDENTIFIED BY '$DB_PASSWORD';
GRANT ALL PRIVILEGES ON $DB_NAME.* TO '$DB_USER'@'localhost';
FLUSH PRIVILEGES;

USE $DB_NAME;

CREATE TABLE files (
    id INT AUTO_INCREMENT PRIMARY KEY,
    post_id VARCHAR(255) NOT NULL,
    parent_id INT,
    title VARCHAR(30) NOT NULL,
    message TEXT NOT NULL,
    file_path VARCHAR(255),
    last_reply_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

MYSQL_SCRIPT

echo "Database and user setup completed."
