# 1.  Se logger sur l'ordi
-> c'est ça qui permet de confirmer qu'on est plus dans la liste d'attente

# 2.  Importer le fichier
Soit via une clé USB, soit via MyFiles (VM)

# 3. Config sur Lightburn

## Format 
.dxf ou .svg pour les images. Dans tous les cas il faut un format vectoriel et non pixelisé (~~.png~~)
On peut l'exporter depuis une esquisse sur Fusion360 (et autres logiciels de CAD)
-> ajouter le fichier en drag and drop

## Calques
Définit les paramètres pour chaque courbe du modèle. Définit également l'ordre de découpe (toujours graver puis découper, pas l'inverse).
### mode
ligne -> fais l'extérieur, à sélectionner pour découper
remplissage -> passe à l'intérieur, à sélectionner pour graver

### speed / power
Utiliser le fichier de config (onglet bibliothèque en bas à droite)
-> choisir le matériau
-> choisir la bonne épaisseur si cut

# 4. Config.  sur la machine
avant d'ouvrir : reculer la tête le plus loin possible (pour ne pas l'abîmer)
## 4.1 Réglage des axes x,y
Avec les boutons sur la machine directement

## 4.2 Calibration de l'axe z
aller au milieu de la plaque puis calibrer (\[shift]+\[z+] puis \[enter] sur la grosse, \[focus] sur la petite)

## 4.3 Définir l'origin
placer au bon endroit sur la plaque, puis bouton \[origin]

# 5. Préparation (ordi + machine)
- Sélectionner la pièce, l'origine est sur le point vert. On peut le modifier sur le panel au milieu à droite (souvent mis en haut à gauche)
- Est-ce que la pièce fit sur la plaque ? mode "Cadrer" rond ou rectangulaire, vérifier que le contour est bien sur la pièce

# 6. Lancer la découpe
-> Faire une preview
-> Appuyer sur le bouton vert

# Tips
- Préparer lightburn avant sur https://make.epfl.ch/lightburn-remote 
- Pour faire des boites : boxes.py
- Le buzzer bleu sonne au bureau pour appeler de l'aide
- Problème de découpe : petit bouton stop, problème de machine : gros bouton stop latéral
- après la gravure : poncer légèrement ou nettoyer la pièce
- plaques dans la machines -> gratuites, sinon demander aux coach