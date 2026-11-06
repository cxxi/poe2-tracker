-- Maps faites en groupe : ce que chaque installation sait de l'instance (code et graine) où elle
-- est. Gardé deux jours ; sert à montrer aux autres joueurs de la même instance, et à ceux avec
-- qui l'on a fait une map ces dernières heures, la map en cours.
CREATE TABLE map_presence (
    install_id BIGINT UNSIGNED NOT NULL,
    code VARCHAR(80) NOT NULL,
    seed BIGINT UNSIGNED NOT NULL,
    data TEXT NOT NULL,
    completed BOOLEAN NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (install_id, code, seed),
    KEY map_instance (code, seed),
    KEY map_updated (updated_at),
    CONSTRAINT map_presence_install FOREIGN KEY (install_id) REFERENCES installs (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
