-- Compte Path of Exile de chaque installation (« Pseudo#1234 »), donné par le joueur dans l'app :
-- les profils se lisent par compte et personnage. Collation insensible à la casse, comme le jeu.
ALTER TABLE installs
    ADD COLUMN account VARCHAR(40) NULL,
    ADD INDEX installs_account (account);
