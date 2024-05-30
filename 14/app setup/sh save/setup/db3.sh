#!/bin/bash

DB_NAME="my_database"
DB_USER="my_user"
DB_PASSWORD="my_password"
ROOT_USER="root"

echo "Enter action (add/delete/reset):"
read action
echo "Enter board name:"
read board_name

case $action in
    add)
        sudo mysql -u $ROOT_USER -p <<MYSQL_SCRIPT
        USE $DB_NAME;
        CREATE TABLE IF NOT EXISTS ${board_name}_files (
            id INT AUTO_INCREMENT PRIMARY KEY,
            post_id VARCHAR(255) NOT NULL,
            parent_id INT,
            title VARCHAR(30) NOT NULL,
            message TEXT NOT NULL,
            file_path VARCHAR(255),
            last_reply_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
        );
MYSQL_SCRIPT
        echo "Board $board_name created."
        ;;
    delete)
        sudo mysql -u $ROOT_USER -p <<MYSQL_SCRIPT
        USE $DB_NAME;
        DROP TABLE IF EXISTS ${board_name}_files;
MYSQL_SCRIPT
        echo "Board $board_name deleted."
        ;;
    reset)
        sudo mysql -u $ROOT_USER -p <<MYSQL_SCRIPT
        DROP DATABASE IF EXISTS $DB_NAME;
        CREATE DATABASE $DB_NAME;
        CREATE USER IF NOT EXISTS '$DB_USER'@'localhost' IDENTIFIED BY '$DB_PASSWORD';
        GRANT ALL PRIVILEGES ON $DB_NAME.* TO '$DB_USER'@'localhost';
        FLUSH PRIVILEGES;
MYSQL_SCRIPT
        echo "Database and user setup completed. You can now add boards."
        ;;
    *)
        echo "Invalid action. Please enter add, delete, or reset."
        ;;
esac
